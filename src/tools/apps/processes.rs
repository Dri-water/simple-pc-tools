//! A snapshot of running processes, shared by every probe and action in a check.

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Selects processes by image name, plus optional path and command-line filters
/// (case-insensitive substrings).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ProcessMatch {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmdline_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmdline_excludes: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct Entry {
    pid: u32,
    name: String,
    path: String,
    cmdline: String,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessTable {
    entries: Vec<Entry>,
}

impl ProcessTable {
    pub fn snapshot() -> Self {
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::Always)
                .with_exe(UpdateKind::Always),
        );
        let entries = sys
            .processes()
            .iter()
            .map(|(pid, p)| Entry {
                pid: pid.as_u32(),
                name: p.name().to_string_lossy().to_lowercase(),
                path: p
                    .exe()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default(),
                cmdline: p
                    .cmd()
                    .iter()
                    .map(|a| a.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase(),
            })
            .collect();
        Self { entries }
    }

    /// Every process with this image name (case-insensitive).
    pub fn pids_named(&self, name: &str) -> Vec<u32> {
        let name = name.to_lowercase();
        self.entries
            .iter()
            .filter(|e| e.name == name)
            .map(|e| e.pid)
            .collect()
    }

    pub fn find(&self, m: &ProcessMatch) -> Vec<u32> {
        let name = m.name.to_lowercase();
        let lower = |s: &Option<String>| s.as_ref().map(|s| s.to_lowercase());
        let (path, cmd, excl) = (
            lower(&m.path_contains),
            lower(&m.cmdline_contains),
            lower(&m.cmdline_excludes),
        );
        self.entries
            .iter()
            .filter(|e| e.name == name)
            .filter(|e| path.as_ref().is_none_or(|p| e.path.contains(p)))
            .filter(|e| cmd.as_ref().is_none_or(|c| e.cmdline.contains(c)))
            .filter(|e| excl.as_ref().is_none_or(|x| !e.cmdline.contains(x)))
            .map(|e| e.pid)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> ProcessTable {
        let e = |pid, name: &str, path: &str, cmdline: &str| Entry {
            pid,
            name: name.into(),
            path: path.into(),
            cmdline: cmdline.into(),
        };
        ProcessTable {
            entries: vec![
                e(
                    1,
                    "python.exe",
                    "c:\\py\\python.exe",
                    "python -m my_service",
                ),
                e(
                    2,
                    "python.exe",
                    "c:\\py\\python.exe",
                    "python -m my_service --fake",
                ),
                e(3, "tool.exe", "c:\\program files\\tool\\tool.exe", ""),
                e(4, "tool.exe", "c:\\users\\x\\downloads\\tool.exe", ""),
            ],
        }
    }

    #[test]
    fn filters_by_cmdline_and_exclusion() {
        let m = ProcessMatch {
            name: "Python.exe".into(),
            path_contains: None,
            cmdline_contains: Some("my_service".into()),
            cmdline_excludes: Some("--fake".into()),
        };
        assert_eq!(table().find(&m), vec![1]);
    }

    #[test]
    fn filters_by_path() {
        let m = ProcessMatch {
            name: "tool.exe".into(),
            path_contains: Some("Program Files\\Tool".into()),
            cmdline_contains: None,
            cmdline_excludes: None,
        };
        assert_eq!(table().find(&m), vec![3]);
    }
}
