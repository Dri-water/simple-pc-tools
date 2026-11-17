//! The tools, and the one list that registers them.
//!
//! Each tool is a module here with its own JSON API (nested under
//! `/api/<id>`) and a page script in `ui/js/<id>.js`. Settings, the installer's
//! `--tools` choice, the nav and the Settings page all read `ALL`, so a new
//! tool is added in one place on each side. See CONTRIBUTING.md, "Adding a
//! tool"; `tests/ui_design.rs` checks that every tool listed here has a page,
//! an icon, demo data and an installer component.

pub mod apps;
pub mod disk;
pub mod resources;

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::Router;

use crate::settings::Settings;

/// A tool as the user sees it: in the nav, on the Settings page and in the installer.
pub struct ToolInfo {
    /// Stable id: the settings key, the `--tools` name, `/api/<id>` and `ui/js/<id>.js`.
    pub id: &'static str,
    pub name: &'static str,
    /// One sentence for Settings and the installer.
    pub about: &'static str,
}

pub const ALL: &[ToolInfo] = &[
    ToolInfo {
        id: "apps",
        name: "Apps",
        about: "Start programs when you sign in and keep them running.",
    },
    ToolInfo {
        id: "disk",
        name: "Disk usage",
        about: "See which folders take up the most space.",
    },
    ToolInfo {
        id: "resources",
        name: "Resources",
        about: "See what's using your memory, processor and network, and which program has a port open.",
    },
];

pub fn find(id: &str) -> Option<&'static ToolInfo> {
    ALL.iter().find(|t| t.id == id)
}

/// The running tools. Built once at start-up; cheap to clone.
#[derive(Clone)]
pub struct Tools {
    pub apps: Arc<apps::supervisor::Manager>,
    pub disk: Arc<disk::Disk>,
    pub resources: Arc<resources::Resources>,
}

impl Tools {
    /// Loads each tool's saved state and starts its background work on the
    /// current Tokio runtime.
    pub fn start(data_dir: &Path, settings: &Settings) -> Self {
        let tools = Self {
            apps: apps::start(data_dir),
            disk: Arc::default(),
            resources: resources::start(),
        };
        for t in ALL {
            tools.set_enabled(t.id, settings.tool_on(t.id));
        }
        tools
    }

    /// Every tool's API, each under `/api/<id>`.
    pub fn router(&self) -> Router {
        Router::new()
            .nest("/api/apps", apps::api::router(self.apps.clone()))
            .nest("/api/disk", disk::router(self.disk.clone()))
            .nest("/api/resources", resources::router(self.resources.clone()))
    }

    /// Called when a tool is switched on or off in Settings. Tools with
    /// background work pause it here; page-only tools need nothing.
    pub fn set_enabled(&self, id: &str, on: bool) {
        let (enabled, wake) = match id {
            "apps" => (&self.apps.enabled, &self.apps.wake),
            "resources" => (&self.resources.enabled, &self.resources.wake),
            _ => return,
        };
        enabled.store(on, Ordering::Relaxed);
        wake.notify_one();
    }
}

/// Opens File Explorer with this file or folder selected.
pub fn show_in_explorer(path: &Path) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    if !path.exists() {
        anyhow::bail!("{} no longer exists.", path.display());
    }
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()?;
    Ok(())
}
