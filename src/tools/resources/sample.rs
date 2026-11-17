//! Reads what the PC and each process are using, straight from Windows.
//!
//! A [`Sampler`] keeps a handle to every process it has seen, so each sample
//! only opens new processes. While we hold a handle, Windows can't reuse that
//! process id, so a pid in the process list always means the same process.

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::time::Instant;

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
};
use windows_sys::Win32::System::SystemInformation::{
    GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows_sys::Win32::System::Threading::{
    GetProcessIoCounters, GetProcessTimes, GetSystemTimes, OpenProcess, QueryFullProcessImageNameW,
    TerminateProcess, IO_COUNTERS, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};

/// The whole PC at one moment.
#[derive(Debug, Clone, Default)]
pub struct System {
    /// Processor use, 0–100 % of all cores.
    pub cpu: f32,
    pub mem_total: u64,
    pub mem_used: u64,
    pub commit_used: u64,
    pub commit_limit: u64,
    /// Bytes per second received and sent, over all network adapters.
    pub net_rx: f64,
    pub net_tx: f64,
    pub uptime_secs: u64,
}

/// One process at one moment.
#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    pub parent: u32,
    /// The image file name, e.g. `chrome.exe`.
    pub name: String,
    /// The full image path, when Windows lets us read it.
    pub path: Option<String>,
    pub threads: u32,
    /// Private working set: the memory only this process uses (what Task
    /// Manager shows), so the processes add up without double counting.
    pub mem: u64,
    /// 0–100 % of all cores.
    pub cpu: f32,
    /// Bytes per second read and written (files, network and devices).
    pub io: f64,
}

struct Tracked {
    handle: Handle,
    path: Option<String>,
    cpu_time: u64,
    io_bytes: u64,
}

struct Handle(HANDLE);
// A process handle is just a kernel object reference; it can move between threads.
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

#[derive(Default)]
pub struct Sampler {
    procs: HashMap<u32, Tracked>,
    last: Option<Instant>,
    /// (busy, total) processor time at the last sample.
    sys_cpu: Option<(u64, u64)>,
    net: Option<(u64, u64)>,
}

impl Sampler {
    /// Reads everything. Rates (processor, I/O, network) are since the last
    /// call, so the first call reports them as zero.
    pub fn sample(&mut self) -> (System, Vec<Process>) {
        let now = Instant::now();
        let secs = self.last.map(|l| (now - l).as_secs_f64()).unwrap_or(0.0);
        self.last = Some(now);
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;

        let mut sys = System {
            uptime_secs: unsafe { GetTickCount64() } / 1000,
            ..Default::default()
        };
        unsafe {
            let mut m: MEMORYSTATUSEX = zeroed();
            m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut m) != 0 {
                sys.mem_total = m.ullTotalPhys;
                sys.mem_used = m.ullTotalPhys - m.ullAvailPhys;
                sys.commit_limit = m.ullTotalPageFile;
                sys.commit_used = m.ullTotalPageFile - m.ullAvailPageFile;
            }
        }
        if let Some((busy, total)) = system_times() {
            if let Some((b0, t0)) = self.sys_cpu {
                if total > t0 {
                    sys.cpu = ((busy - b0) as f64 / (total - t0) as f64 * 100.0) as f32;
                }
            }
            self.sys_cpu = Some((busy, total));
        }
        if let Some((rx, tx)) = network_bytes() {
            if let (Some((r0, t0)), true) = (self.net, secs > 0.0) {
                sys.net_rx = rx.saturating_sub(r0) as f64 / secs;
                sys.net_tx = tx.saturating_sub(t0) as f64 / secs;
            }
            self.net = Some((rx, tx));
        }

        let list = process_list();
        let alive: std::collections::HashSet<u32> = list.iter().map(|e| e.0).collect();
        self.procs.retain(|pid, _| alive.contains(pid));

        let mut out = Vec::with_capacity(list.len());
        for (pid, parent, threads, name) in list {
            if pid == 0 {
                continue; // "System Idle Process" is the idle time, not a program.
            }
            let fresh = !self.procs.contains_key(&pid);
            if fresh {
                let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
                if !h.is_null() {
                    let handle = Handle(h);
                    let path = image_path(h);
                    self.procs.insert(
                        pid,
                        Tracked {
                            handle,
                            path,
                            cpu_time: 0,
                            io_bytes: 0,
                        },
                    );
                }
            }
            let mut p = Process {
                pid,
                parent,
                name,
                path: None,
                threads,
                mem: 0,
                cpu: 0.0,
                io: 0.0,
            };
            if let Some(t) = self.procs.get_mut(&pid) {
                let h = t.handle.0;
                p.path = t.path.clone();
                p.mem = private_memory(h);
                let cpu_time = process_cpu_time(h);
                let io_bytes = process_io_bytes(h);
                if !fresh && secs > 0.0 {
                    p.cpu = (cpu_time.saturating_sub(t.cpu_time) as f64 / (secs * 1e7 * cores)
                        * 100.0)
                        .min(100.0) as f32;
                    p.io = io_bytes.saturating_sub(t.io_bytes) as f64 / secs;
                }
                t.cpu_time = cpu_time;
                t.io_bytes = io_bytes;
            }
            out.push(p);
        }
        (sys, out)
    }
}

/// Ends a process. `Err` holds a plain reason.
pub fn end_process(pid: u32) -> Result<(), String> {
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if h.is_null() {
            return Err(match std::io::Error::last_os_error().raw_os_error() {
                Some(5) => "Windows didn't allow it (access denied).".into(),
                Some(87) => "it has already closed.".into(),
                _ => std::io::Error::last_os_error().to_string(),
            });
        }
        let h = Handle(h);
        if TerminateProcess(h.0, 1) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok(())
}

/// The "File description" from an exe's version info, e.g. "Google Chrome".
pub fn file_description(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let query = |sub: &str| -> Option<(*const c_void, u32)> {
            let sub: Vec<u16> = sub.encode_utf16().chain([0]).collect();
            let mut ptr = std::ptr::null_mut();
            let mut len = 0u32;
            (VerQueryValueW(data.as_ptr().cast(), sub.as_ptr(), &mut ptr, &mut len) != 0
                && !ptr.is_null()
                && len > 0)
                .then_some((ptr as *const c_void, len))
        };
        // The translations the file has, then the common English ones.
        let mut langs = Vec::new();
        if let Some((ptr, len)) = query("\\VarFileInfo\\Translation") {
            let pairs = std::slice::from_raw_parts(ptr as *const u16, len as usize / 2);
            for pair in pairs.as_chunks::<2>().0 {
                langs.push(format!("{:04x}{:04x}", pair[0], pair[1]));
            }
        }
        langs.extend(["040904b0".into(), "040904e4".into(), "04090000".into()]);
        for lang in langs {
            if let Some((ptr, len)) = query(&format!("\\StringFileInfo\\{lang}\\FileDescription")) {
                let text = std::slice::from_raw_parts(ptr as *const u16, len as usize);
                let text = String::from_utf16_lossy(text);
                let text = text.trim_end_matches('\0').trim();
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

fn filetime(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

/// (busy, total) processor time over all cores, in 100 ns units.
fn system_times() -> Option<(u64, u64)> {
    unsafe {
        let (mut idle, mut kernel, mut user) = (zeroed(), zeroed(), zeroed());
        if GetSystemTimes(&mut idle, &mut kernel, &mut user) == 0 {
            return None;
        }
        // Kernel time includes idle time.
        let total = filetime(kernel) + filetime(user);
        Some((total - filetime(idle), total))
    }
}

/// Total bytes (received, sent) over the PC's real network adapters.
fn network_bytes() -> Option<(u64, u64)> {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table) != 0 || table.is_null() {
            return None;
        }
        let rows = std::slice::from_raw_parts(
            (*table).Table.as_ptr() as *const MIB_IF_ROW2,
            (*table).NumEntries as usize,
        );
        let (mut rx, mut tx) = (0, 0);
        for r in rows {
            let flags = r.InterfaceAndOperStatusFlags._bitfield;
            let hardware = flags & 1 != 0;
            let filter = flags & 2 != 0; // a filter layered on another adapter: same bytes again
            if hardware
                && !filter
                && r.OperStatus == IfOperStatusUp
                && r.Type != IF_TYPE_SOFTWARE_LOOPBACK
            {
                rx += r.InOctets;
                tx += r.OutOctets;
            }
        }
        FreeMibTable(table as *const c_void);
        Some((rx, tx))
    }
}

/// (pid, parent pid, threads, image name) for every running process.
fn process_list() -> Vec<(u32, u32, u32, String)> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let snap = Handle(snap);
        let mut e: PROCESSENTRY32W = zeroed();
        e.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap.0, &mut e) != 0;
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
            out.push((
                e.th32ProcessID,
                e.th32ParentProcessID,
                e.cntThreads,
                String::from_utf16_lossy(&e.szExeFile[..len]),
            ));
            ok = Process32NextW(snap.0, &mut e) != 0;
        }
    }
    out
}

fn image_path(h: HANDLE) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe {
        (QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0)
            .then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn private_memory(h: HANDLE) -> u64 {
    unsafe {
        let mut m: PROCESS_MEMORY_COUNTERS_EX2 = zeroed();
        m.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
        if K32GetProcessMemoryInfo(h, (&raw mut m).cast(), m.cb) != 0 {
            return m.PrivateWorkingSetSize as u64;
        }
        // Before Windows 10 1809: the whole working set.
        let mut m: PROCESS_MEMORY_COUNTERS = zeroed();
        m.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if K32GetProcessMemoryInfo(h, &mut m, m.cb) != 0 {
            return m.WorkingSetSize as u64;
        }
    }
    0
}

fn process_cpu_time(h: HANDLE) -> u64 {
    unsafe {
        let (mut c, mut e, mut k, mut u) = (zeroed(), zeroed(), zeroed(), zeroed());
        if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) == 0 {
            return 0;
        }
        filetime(k) + filetime(u)
    }
}

fn process_io_bytes(h: HANDLE) -> u64 {
    unsafe {
        let mut io: IO_COUNTERS = zeroed();
        if GetProcessIoCounters(h, &mut io) == 0 {
            return 0;
        }
        io.ReadTransferCount + io.WriteTransferCount + io.OtherTransferCount
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_this_process_and_the_pc() {
        let mut s = Sampler::default();
        s.sample();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let (sys, procs) = s.sample();
        assert!(sys.mem_total > 0 && sys.mem_used > 0 && sys.mem_used <= sys.mem_total);
        assert!((0.0..=100.0).contains(&sys.cpu));
        let me = procs
            .iter()
            .find(|p| p.pid == std::process::id())
            .expect("this test's own process");
        assert!(me.mem > 0, "private memory of our own process");
        assert!(me.path.as_deref().is_some_and(|p| p.ends_with(".exe")));
        assert!(procs.len() > 10);
    }

    #[test]
    fn reads_file_descriptions() {
        let explorer = std::env::var("WINDIR").unwrap() + "\\explorer.exe";
        let d = file_description(&explorer).expect("explorer.exe has version info");
        assert!(!d.is_empty());
    }
}
