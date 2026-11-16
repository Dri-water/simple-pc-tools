//! "Start with Windows": a Task Scheduler task that starts Simple PC Tools
//! when you sign in, as administrator, without a UAC prompt, and restarts it
//! if it crashes. Creating or removing it needs administrator rights.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

pub const TASK: &str = "Simple PC Tools";

fn schtasks(args: &[&str]) -> Result<std::process::Output> {
    let mut cmd = Command::new("schtasks");
    cmd.args(args).stdin(Stdio::null());
    crate::tools::apps::action::no_window(&mut cmd);
    cmd.output().context("running schtasks")
}

pub fn is_enabled() -> bool {
    schtasks(&["/Query", "/TN", TASK]).is_ok_and(|o| o.status.success())
}

/// Starts the task now (works without a UAC prompt from a normal process).
pub fn run_now() -> Result<()> {
    let out = schtasks(&["/Run", "/TN", TASK])?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

pub fn enable(exe: &Path) -> Result<()> {
    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(d), Ok(u)) => format!("{d}\\{u}"),
        (_, Ok(u)) => u,
        _ => bail!("can't tell who is signed in"),
    };
    let xml = task_xml(&user, &exe.display().to_string());
    // schtasks reads task XML as UTF-16 with a byte-order mark.
    let mut bytes = vec![0xFF, 0xFE];
    for unit in xml.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let file = std::env::temp_dir().join("simple-pc-tools-task.xml");
    std::fs::write(&file, bytes)?;
    let out = schtasks(&[
        "/Create",
        "/TN",
        TASK,
        "/XML",
        &file.display().to_string(),
        "/F",
    ]);
    let _ = std::fs::remove_file(&file);
    let out = out?;
    if !out.status.success() {
        bail!(
            "couldn't turn on Start with Windows: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

pub fn disable() -> Result<()> {
    if !is_enabled() {
        return Ok(());
    }
    let out = schtasks(&["/Delete", "/TN", TASK, "/F"])?;
    if !out.status.success() {
        bail!(
            "couldn't turn off Start with Windows: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn task_xml(user: &str, exe: &str) -> String {
    let (user, exe) = (escape(user), escape(exe));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts Simple PC Tools when you sign in.</Description>
    <URI>\{TASK}</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{exe}"</Command>
      <Arguments>--background</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn task_xml_escapes_and_runs_elevated_at_logon() {
        let xml = super::task_xml(
            "PC\\Sam & Co",
            r"C:\Program Files\Simple PC Tools\simple-pc-tools.exe",
        );
        assert!(xml.contains("<UserId>PC\\Sam &amp; Co</UserId>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<Arguments>--background</Arguments>"));
    }
}
