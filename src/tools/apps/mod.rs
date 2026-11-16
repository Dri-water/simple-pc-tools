//! App manager: starts the programs you pick when you sign in, checks on them
//! and restarts the ones that stop or stop answering.
//!
//! Basic use is a program path and two switches. Everything else (custom
//! health checks such as "this URL must answer", hold conditions, restart
//! limits, custom start/stop steps) lives under `advanced` and has defaults.

pub mod action;
pub mod api;
mod env;
mod launch;
pub mod probe;
pub mod processes;
pub mod supervisor;

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use action::Action;
use probe::Probe;
use processes::ProcessMatch;

/// Everything the app manager saves (`apps.json` in the data folder).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Store {
    /// Check, but never start, stop or restart anything.
    #[serde(default)]
    pub paused: bool,
    /// Seconds between checks while everything is fine.
    #[serde(default = "sixty")]
    pub check_every_secs: u64,
    #[serde(default)]
    pub apps: Vec<App>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct App {
    pub id: String,
    pub name: String,
    /// The program to start: an .exe, a script, a shortcut or any file.
    pub program: String,
    /// The rest of the command line.
    #[serde(default)]
    pub args: String,
    /// Folder to start in; defaults to the program's folder.
    #[serde(default)]
    pub folder: String,
    /// Start it when you sign in, if it isn't already running.
    #[serde(default = "yes")]
    pub start_at_login: bool,
    /// Check it regularly and restart it when it stops or fails a check.
    #[serde(default = "yes")]
    pub keep_running: bool,
    #[serde(default)]
    pub advanced: Advanced,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Advanced {
    /// Process that shows the app is running. Default: the program itself
    /// (for .exe programs). Set it when the program is a launcher or script
    /// that starts something else.
    pub process: Option<ProcessMatch>,
    /// Skip the "process is running" check and rely on `checks` only.
    pub skip_process_check: bool,
    /// Extra checks that must all pass, e.g. a URL that must answer.
    pub checks: Vec<Probe>,
    /// When any of these pass, leave the app alone even if it looks unhealthy
    /// (it is busy, or you stopped it on purpose).
    pub hold_if: Vec<Probe>,
    /// When the process runs but a check fails, end it before starting again.
    pub end_before_restart: bool,
    pub run_as_admin: bool,
    /// Start console programs without a window.
    pub hide_window: bool,
    /// Failed checks in a row before restarting.
    pub failures_before_restart: u32,
    /// Seconds after a start during which failed checks don't count.
    pub startup_grace_secs: u64,
    /// Stop retrying (until restarted by hand) after this many restarts in an hour.
    pub max_restarts_per_hour: u32,
    /// Replace the default start step (run `program`) with these.
    pub custom_start: Vec<Action>,
    /// Replace the default stop step (end the process) with these.
    pub custom_stop: Vec<Action>,
}

impl Default for Advanced {
    fn default() -> Self {
        Self {
            process: None,
            skip_process_check: false,
            checks: Vec::new(),
            hold_if: Vec::new(),
            end_before_restart: true,
            run_as_admin: false,
            hide_window: false,
            failures_before_restart: 2,
            startup_grace_secs: 60,
            max_restarts_per_hour: 5,
            custom_start: Vec::new(),
            custom_stop: Vec::new(),
        }
    }
}

fn yes() -> bool {
    true
}
fn sixty() -> u64 {
    60
}

/// What the supervisor does for one app, worked out from its settings.
#[derive(Debug, Clone)]
pub struct Plan {
    pub health: Vec<Probe>,
    pub hold_if: Vec<Probe>,
    pub stop: Vec<Action>,
    pub start: Vec<Action>,
}

impl App {
    /// The process that shows this app is running, if one can be known.
    pub fn process(&self) -> Option<ProcessMatch> {
        if let Some(p) = &self.advanced.process {
            return Some(p.clone());
        }
        let path = Path::new(self.program.trim_matches('"'));
        let is_exe = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        is_exe.then(|| ProcessMatch {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            path_contains: path.is_absolute().then(|| path.display().to_string()),
            cmdline_contains: None,
            cmdline_excludes: None,
        })
    }

    pub fn plan(&self) -> Plan {
        let a = &self.advanced;
        let process = self.process();
        let mut health = Vec::new();
        if !a.skip_process_check {
            if let Some(p) = &process {
                health.push(Probe::Process(p.clone()));
            }
        }
        health.extend(a.checks.iter().cloned());
        let start = if a.custom_start.is_empty() {
            vec![Action::Run {
                program: self.program.clone(),
                args: self.args.clone(),
                folder: (!self.folder.trim().is_empty()).then(|| self.folder.clone()),
                hidden: a.hide_window,
            }]
        } else {
            a.custom_start.clone()
        };
        let stop = if !a.custom_stop.is_empty() {
            a.custom_stop.clone()
        } else if let (true, Some(p)) = (a.end_before_restart, process) {
            vec![
                Action::EndProcess {
                    target: p,
                    tree: true,
                },
                Action::Wait { secs: 2 },
            ]
        } else {
            Vec::new()
        };
        Plan {
            health,
            hold_if: a.hold_if.clone(),
            stop,
            start,
        }
    }

    /// Problems that make the app unusable, in words for the UI.
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("Give the app a name.");
        }
        if self.program.trim().is_empty() && self.advanced.custom_start.is_empty() {
            bail!("Choose the program to start.");
        }
        if self.keep_running && self.plan().health.is_empty() {
            bail!(
                "\"Keep running\" needs a way to tell the app is running. This program isn't an .exe, \
                 so add a check under Advanced (the process to look for, or a URL or port), or turn \
                 \"Keep running\" off."
            );
        }
        let a = &self.advanced;
        if a.failures_before_restart == 0 || a.max_restarts_per_hour == 0 {
            bail!("Restart limits must be at least 1.");
        }
        Ok(())
    }
}

impl Store {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                check_every_secs: sixty(),
                ..Self::default()
            }),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        crate::settings::write_atomic(path, &serde_json::to_vec_pretty(self)?)
    }

    pub fn app(&self, id: &str) -> Option<&App> {
        self.apps.iter().find(|a| a.id == id)
    }

    /// A new unique id made from the name ("My App" -> "my-app", "my-app-2").
    pub fn new_id(&self, name: &str) -> String {
        let base: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let base = if base.is_empty() {
            "app".to_string()
        } else {
            base
        };
        let mut id = base.clone();
        let mut n = 2;
        while self.app(&id).is_some() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }
}

pub fn store_path(data_dir: &Path) -> PathBuf {
    data_dir.join("apps.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(program: &str) -> App {
        serde_json::from_value(serde_json::json!({
            "id": "a", "name": "A", "program": program
        }))
        .unwrap()
    }

    #[test]
    fn exe_gets_a_process_check_and_a_stop_step() {
        let p = app(r"C:\Tools\server.exe").plan();
        assert!(matches!(&p.health[..], [Probe::Process(m)] if m.name == "server.exe"));
        assert!(matches!(p.stop[0], Action::EndProcess { .. }));
        assert!(
            matches!(&p.start[..], [Action::Run { program, .. }] if program == r"C:\Tools\server.exe")
        );
    }

    #[test]
    fn script_needs_a_check_to_keep_running() {
        let mut a = app(r"C:\Tools\start.bat");
        assert!(a.validate().is_err());
        a.keep_running = false;
        assert!(a.validate().is_ok());
        a.keep_running = true;
        a.advanced.checks.push(Probe::Tcp {
            host: "127.0.0.1".into(),
            port: 80,
            timeout_secs: 3,
        });
        assert!(a.validate().is_ok());
    }

    #[test]
    fn ids_are_unique_and_readable() {
        let mut s = Store::default();
        assert_eq!(s.new_id("My App!"), "my-app");
        s.apps.push(App {
            id: "my-app".into(),
            ..app("x.exe")
        });
        assert_eq!(s.new_id("My App"), "my-app-2");
        assert_eq!(s.new_id("???"), "app");
    }

    #[test]
    fn minimal_json_gets_defaults() {
        let a = app("x.exe");
        assert!(a.start_at_login && a.keep_running);
        assert_eq!(a.advanced, Advanced::default());
    }
}
