//! Simple PC Tools runs as administrator (the fast disk scan and starting
//! apps for the user need it). A normal launch hands over to the sign-in
//! task, which runs elevated without a UAC prompt; without the task it asks
//! Windows to relaunch it elevated (one UAC prompt).

use anyhow::{bail, Result};

pub fn is_elevated() -> bool {
    disk_scan::is_elevated()
}

/// Starts this exe again as administrator with `args`. Fails if the user
/// declines the UAC prompt.
#[cfg(windows)]
pub fn relaunch_elevated(args: &[String]) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let exe = wide(std::env::current_exe()?.as_os_str());
    let params = args
        .iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let params = wide(std::ffi::OsStr::new(&params));
    let verb = wide(std::ffi::OsStr::new("runas"));
    // SAFETY: zeroed struct with cbSize set; all strings are NUL-terminated and outlive the call.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = exe.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        if ShellExecuteExW(&mut info) == 0 {
            bail!(
                "Windows didn't start Simple PC Tools as administrator (the prompt was declined?)"
            );
        }
    }
    Ok(())
}
