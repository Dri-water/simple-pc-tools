//! Thin wrappers around the Win32 calls the scanners need.

use std::ffi::{c_void, OsStr};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetCompressedFileSizeW, GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetDriveTypeW,
    GetFileInformationByHandle, GetLogicalDrives, GetVolumeInformationW,
    GetVolumeNameForVolumeMountPointW, GetVolumePathNameW, ReadFile, BY_HANDLE_FILE_INFORMATION,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, INVALID_FILE_SIZE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Diagnostics::Debug::{SetThreadErrorMode, SEM_FAILCRITICALERRORS};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::System::IO::OVERLAPPED;

use crate::Drive;

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

/// NUL-terminated UTF-16 copy of `s`.
pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// An owned Win32 handle, closed on drop.
pub struct Handle(HANDLE);

// SAFETY: a file handle can be used from any thread; reads pass explicit offsets.
unsafe impl Send for Handle {}
// SAFETY: as above; ReadFile with an OVERLAPPED offset does not share a file pointer.
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW/OpenProcessToken and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

impl Handle {
    fn open(path: &OsStr, access: u32, flags: u32) -> io::Result<Handle> {
        let w = wide(path);
        // SAFETY: `w` is a NUL-terminated path that outlives the call.
        let h = unsafe {
            CreateFileW(
                w.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                flags,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Handle(h))
        }
    }

    /// Opens a volume (`\\?\Volume{...}` or `\\.\C:`) for raw reading.
    pub fn open_volume(device: &str) -> io::Result<Handle> {
        Handle::open(OsStr::new(device), GENERIC_READ, 0)
    }

    /// Reads `buf.len()` bytes at `offset`. On a volume both must be
    /// multiples of the sector size.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let mut done = 0usize;
        while done < buf.len() {
            let pos = offset + done as u64;
            let want = (buf.len() - done).min(1 << 30) as u32;
            // SAFETY: OVERLAPPED is plain data; all-zero is a valid value.
            let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
            ov.Anonymous.Anonymous.Offset = pos as u32;
            ov.Anonymous.Anonymous.OffsetHigh = (pos >> 32) as u32;
            let mut got = 0u32;
            // SAFETY: the buffer range is valid for `want` bytes; the handle is synchronous,
            // so the call completes before returning and `ov` only carries the offset.
            let ok = unsafe { ReadFile(self.0, buf[done..].as_mut_ptr(), want, &mut got, &mut ov) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            if got == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "volume read past end",
                ));
            }
            done += got as usize;
        }
        Ok(())
    }
}

/// Identity of a file or folder: volume serial and NTFS file reference.
pub fn file_id(path: &Path) -> io::Result<(u32, u64)> {
    let h = Handle::open(
        path.as_os_str(),
        FILE_READ_ATTRIBUTES,
        FILE_FLAG_BACKUP_SEMANTICS,
    )?;
    // SAFETY: plain data, zero is valid.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: valid handle and out-pointer.
    if unsafe { GetFileInformationByHandle(h.0, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let frn = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((info.dwVolumeSerialNumber, frn))
}

/// Mount point of the volume holding `path`, e.g. `C:\`.
pub fn volume_path(path: &Path) -> io::Result<PathBuf> {
    let w = wide(path);
    let mut buf = vec![0u16; 1024];
    // SAFETY: `w` is NUL-terminated; `buf` holds `buf.len()` u16s.
    if unsafe { GetVolumePathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(from_wide(&buf)))
}

/// `\\?\Volume{GUID}\` for a mount point like `C:\`.
pub fn volume_guid_path(mount: &Path) -> io::Result<String> {
    let w = wide(mount);
    let mut buf = [0u16; 128];
    // SAFETY: `w` is NUL-terminated; `buf` holds 128 u16s.
    if unsafe { GetVolumeNameForVolumeMountPointW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(from_wide(&buf))
}

pub struct VolumeInfo {
    pub label: String,
    pub file_system: String,
    pub serial: u32,
}

/// Label, file system and serial of the volume mounted at `root` (ends with `\`).
pub fn volume_info(root: &Path) -> io::Result<VolumeInfo> {
    let w = wide(root);
    let mut label = [0u16; 261];
    let mut fs = [0u16; 261];
    let mut serial = 0u32;
    let (mut maxlen, mut flags) = (0u32, 0u32);
    // SAFETY: all buffers are sized as passed; out-pointers are valid.
    let ok = unsafe {
        GetVolumeInformationW(
            w.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            &mut serial,
            &mut maxlen,
            &mut flags,
            fs.as_mut_ptr(),
            fs.len() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(VolumeInfo {
        label: from_wide(&label),
        file_system: from_wide(&fs),
        serial,
    })
}

/// Whether the volume at `root` is a fixed or removable local disk.
pub fn is_local(root: &Path) -> bool {
    let w = wide(root);
    // SAFETY: `w` is NUL-terminated.
    let t = unsafe { GetDriveTypeW(w.as_ptr()) };
    t == DRIVE_FIXED || t == DRIVE_REMOVABLE
}

/// Cluster size in bytes of the volume at `root`, if known.
pub fn cluster_size(root: &Path) -> Option<u64> {
    let w = wide(root);
    let (mut spc, mut bps, mut free, mut total) = (0u32, 0u32, 0u32, 0u32);
    // SAFETY: `w` is NUL-terminated; out-pointers are valid.
    let ok = unsafe { GetDiskFreeSpaceW(w.as_ptr(), &mut spc, &mut bps, &mut free, &mut total) };
    let c = u64::from(spc) * u64::from(bps);
    (ok != 0 && c > 0).then_some(c)
}

/// On-disk size of a compressed, sparse or cloud placeholder file.
pub fn compressed_size(path: &Path) -> Option<u64> {
    let w = wide(path);
    let mut high = 0u32;
    // SAFETY: `w` is NUL-terminated; `high` is a valid out-pointer.
    let low = unsafe { GetCompressedFileSizeW(w.as_ptr(), &mut high) };
    if low == INVALID_FILE_SIZE && io::Error::last_os_error().raw_os_error() != Some(0) {
        return None;
    }
    Some((u64::from(high) << 32) | u64::from(low))
}

pub fn drives() -> Vec<Drive> {
    // Never show "insert a disk" dialogs for empty card readers.
    let mut old = 0u32;
    // SAFETY: only changes this thread's error mode; restored below.
    unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS, &mut old) };
    // SAFETY: no arguments.
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = PathBuf::from(format!("{}:\\", (b'A' + i as u8) as char));
        if !is_local(&root) {
            continue;
        }
        let Ok(info) = volume_info(&root) else {
            continue;
        }; // no media
        let w = wide(&root);
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: `w` is NUL-terminated; out-pointers are valid.
        if unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) } == 0 {
            continue;
        }
        out.push(Drive {
            root,
            label: info.label,
            file_system: info.file_system,
            total,
            free,
        });
    }
    // SAFETY: restores the previous mode.
    unsafe { SetThreadErrorMode(old, std::ptr::null_mut()) };
    out
}

pub fn is_elevated() -> bool {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo-handle from GetCurrentProcess needs no closing; `token` is an out-pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let token = Handle(token);
    let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut len = 0u32;
    // SAFETY: `elev` is a TOKEN_ELEVATION of the size passed.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            &mut elev as *mut TOKEN_ELEVATION as *mut c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
    };
    ok != 0 && elev.TokenIsElevated != 0
}
