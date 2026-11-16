//! Actions: ways to start or stop an app. Most apps never need these: the
//! manager builds them from the app's program and settings. Power users can
//! replace them with their own steps in an app's advanced settings.

use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::env::expand;
use super::launch::{self, Launch};
use super::processes::{ProcessMatch, ProcessTable};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Start a program and leave it running on its own.
    Run {
        program: String,
        #[serde(default)]
        args: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        folder: Option<String>,
        /// Run console programs without a window.
        #[serde(default)]
        hidden: bool,
    },
    /// End every matching process (and, by default, its child processes).
    EndProcess {
        #[serde(flatten)]
        target: ProcessMatch,
        #[serde(default = "yes")]
        tree: bool,
    },
    /// Wait before the next step.
    Wait { secs: u64 },
}

fn yes() -> bool {
    true
}

impl Action {
    pub async fn run(&self, as_admin: bool) -> Result<String> {
        match self {
            Action::Run {
                program,
                args,
                folder,
                hidden,
            } => {
                let program = expand(program);
                let args = expand(args);
                let folder = folder.as_deref().map(expand);
                let (p, a, f, h) = (program.clone(), args, folder, *hidden);
                let pid = tokio::task::spawn_blocking(move || {
                    launch::start(&Launch {
                        program: &p,
                        args: &a,
                        folder: f.as_deref(),
                        hidden: h,
                        as_admin,
                    })
                })
                .await??;
                Ok(format!("started {program} (pid {pid})"))
            }
            Action::EndProcess { target, tree } => {
                let pids = ProcessTable::snapshot().find(target);
                for pid in &pids {
                    let mut kill = Command::new("taskkill");
                    kill.args(["/PID", &pid.to_string(), "/F"]);
                    if *tree {
                        kill.arg("/T");
                    }
                    no_window(&mut kill);
                    kill.stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .context("running taskkill")?;
                }
                Ok(format!("ended {} {} process(es)", pids.len(), target.name))
            }
            Action::Wait { secs } => {
                tokio::time::sleep(std::time::Duration::from_secs(*secs)).await;
                Ok(format!("waited {secs}s"))
            }
        }
    }
}

#[cfg(windows)]
pub fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

#[cfg(not(windows))]
pub fn no_window(_cmd: &mut Command) {}
