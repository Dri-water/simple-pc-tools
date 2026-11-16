//! Starting programs. Simple PC Tools runs as administrator, but the apps it
//! starts should normally not: by default they are started with the signed-in
//! user's normal (non-elevated) rights, borrowed from Explorer, exactly as if
//! they had been opened from the Start menu. "Run as administrator" in an
//! app's advanced settings starts it elevated instead.

use std::path::Path;

use anyhow::{bail, Context, Result};

/// What to start and how.
pub struct Launch<'a> {
    /// Program to run: an .exe, or a .bat/.cmd/.ps1 or any file Windows can open.
    pub program: &'a str,
    /// The rest of the command line, written as you would in a shortcut.
    pub args: &'a str,
    pub folder: Option<&'a str>,
    /// No console window for console programs.
    pub hidden: bool,
    pub as_admin: bool,
}

/// Starts the program and returns its process id. The program keeps running
/// on its own: closing or restarting Simple PC Tools never stops it.
pub fn start(l: &Launch) -> Result<u32> {
    let (exe, cmdline) = command_line(l.program, l.args);
    let folder = l
        .folder
        .filter(|f| !f.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            Path::new(l.program)
                .parent()
                .map(|p| p.display().to_string())
        })
        .filter(|f| Path::new(f).is_dir());
    if !l.as_admin && crate::elevation::is_elevated() {
        match win::start_as_user(&exe, &cmdline, folder.as_deref(), l.hidden) {
            Ok(pid) => return Ok(pid),
            Err(e) => bail!("could not start {} with normal rights: {e:#}", l.program),
        }
    }
    win::start_inherit(&exe, &cmdline, folder.as_deref(), l.hidden)
        .with_context(|| format!("starting {}", l.program))
}

/// The executable to run and the full command line. Scripts and documents go
/// through the program Windows would use for them.
fn command_line(program: &str, args: &str) -> (String, String) {
    let ext = Path::new(program)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let quoted = format!("\"{}\"", program.trim_matches('"'));
    let tail = if args.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", args.trim())
    };
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    match ext.as_str() {
        "exe" | "com" => (
            program.trim_matches('"').to_string(),
            format!("{quoted}{tail}"),
        ),
        "bat" | "cmd" => {
            let cmd = format!(r"{system}\System32\cmd.exe");
            (cmd, format!("cmd.exe /d /c \"{quoted}{tail}\""))
        }
        "ps1" => {
            let ps = format!(r"{system}\System32\WindowsPowerShell\v1.0\powershell.exe");
            (
                ps,
                format!("powershell.exe -NoProfile -ExecutionPolicy Bypass -File {quoted}{tail}"),
            )
        }
        // Anything else (shortcuts, documents, URLs): let Explorer's "start" decide.
        _ => {
            let cmd = format!(r"{system}\System32\cmd.exe");
            (cmd, format!("cmd.exe /d /c start \"\" {quoted}{tail}"))
        }
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};

    use anyhow::{bail, Result};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::Security::{
        DuplicateTokenEx, SecurityImpersonation, TokenPrimary, TOKEN_ALL_ACCESS,
        TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, CreateProcessWithTokenW, GetCurrentProcessId, OpenProcess,
        OpenProcessToken, CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
        CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, PROCESS_QUERY_INFORMATION, STARTUPINFOW,
    };

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    fn flags(hidden: bool) -> u32 {
        let mut f = CREATE_NEW_PROCESS_GROUP | CREATE_UNICODE_ENVIRONMENT;
        if hidden {
            f |= CREATE_NO_WINDOW;
        }
        f
    }

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: we own this handle and close it once.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    /// Explorer's process in our session: its token carries the signed-in
    /// user's normal rights.
    fn explorer_pid() -> Option<u32> {
        let mut mine = 0;
        // SAFETY: plain out-parameter.
        unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut mine) };
        let table = super::super::processes::ProcessTable::snapshot();
        table.pids_named("explorer.exe").into_iter().find(|&pid| {
            let mut session = u32::MAX;
            // SAFETY: plain out-parameter.
            let ok = unsafe { ProcessIdToSessionId(pid, &mut session) };
            ok != 0 && session == mine
        })
    }

    pub fn start_as_user(
        exe: &str,
        cmdline: &str,
        folder: Option<&str>,
        hidden: bool,
    ) -> Result<u32> {
        // At sign-in Explorer may not be up yet; give it a moment.
        let mut pid = explorer_pid();
        for _ in 0..30 {
            if pid.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
            pid = explorer_pid();
        }
        let Some(pid) = pid else {
            bail!("Explorer is not running, so there is no user session to start it in")
        };
        // SAFETY: Win32 calls with valid out-pointers; handles are closed by `Handle`.
        unsafe {
            let process = Handle(OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid));
            if process.0.is_null() {
                bail!("opening Explorer failed (error {})", GetLastError());
            }
            let mut token = null_mut();
            if OpenProcessToken(
                process.0,
                TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY,
                &mut token,
            ) == 0
            {
                bail!(
                    "reading Explorer's rights failed (error {})",
                    GetLastError()
                );
            }
            let token = Handle(token);
            let mut primary = null_mut();
            if DuplicateTokenEx(
                token.0,
                TOKEN_ALL_ACCESS,
                null(),
                SecurityImpersonation,
                TokenPrimary,
                &mut primary,
            ) == 0
            {
                bail!(
                    "copying Explorer's rights failed (error {})",
                    GetLastError()
                );
            }
            let primary = Handle(primary);
            let exe = wide(exe);
            let mut cmd = wide(cmdline);
            let folder = folder.map(wide);
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessWithTokenW(
                primary.0,
                0,
                exe.as_ptr(),
                cmd.as_mut_ptr(),
                flags(hidden),
                null(),
                folder.as_ref().map_or(null(), |f| f.as_ptr()),
                &si,
                &mut pi,
            );
            if ok == 0 {
                bail!("error {}", GetLastError());
            }
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            Ok(pi.dwProcessId)
        }
    }

    /// Starts it with our own rights (elevated when we are), outside our job
    /// object when allowed so stopping us never takes it down.
    pub fn start_inherit(
        exe: &str,
        cmdline: &str,
        folder: Option<&str>,
        hidden: bool,
    ) -> Result<u32> {
        let exe = wide(exe);
        let folder = folder.map(wide);
        for breakaway in [true, false] {
            let mut cmd = wide(cmdline);
            let f = flags(hidden)
                | if breakaway {
                    CREATE_BREAKAWAY_FROM_JOB
                } else {
                    0
                };
            // SAFETY: valid, NUL-terminated buffers and zeroed structs.
            unsafe {
                let mut si: STARTUPINFOW = std::mem::zeroed();
                si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
                let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
                let ok = CreateProcessW(
                    exe.as_ptr(),
                    cmd.as_mut_ptr(),
                    null(),
                    null(),
                    0,
                    f,
                    null(),
                    folder.as_ref().map_or(null(), |f| f.as_ptr()),
                    &si,
                    &mut pi,
                );
                if ok != 0 {
                    CloseHandle(pi.hThread);
                    CloseHandle(pi.hProcess);
                    return Ok(pi.dwProcessId);
                }
                if !breakaway {
                    bail!("error {}", GetLastError());
                }
            }
        }
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use super::command_line;

    #[test]
    fn builds_command_lines() {
        let (exe, cmd) = command_line(r"C:\Apps\tool.exe", "--port 80");
        assert_eq!(exe, r"C:\Apps\tool.exe");
        assert_eq!(cmd, r#""C:\Apps\tool.exe" --port 80"#);
        let (exe, cmd) = command_line(r"C:\Apps\run me.bat", "");
        assert!(exe.ends_with(r"\System32\cmd.exe"));
        assert_eq!(cmd, r#"cmd.exe /d /c ""C:\Apps\run me.bat"""#);
    }

    #[test]
    fn starts_a_program() {
        let pid = super::start(&super::Launch {
            program: r"C:\Windows\System32\cmd.exe",
            args: "/c exit 0",
            folder: None,
            hidden: true,
            as_admin: false,
        })
        .unwrap();
        assert!(pid > 0);
    }
}
