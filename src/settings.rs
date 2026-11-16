//! App-wide settings (`settings.json` in the data folder): which tools are
//! on, theme, and update preferences. Each tool keeps its own file.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub tools: Tools,
    /// "system", "light" or "dark".
    pub theme: String,
    pub updates: Updates,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Tools {
    pub apps: bool,
    pub disk: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Updates {
    /// Look for a new release on GitHub once a day. Off = never ask again.
    pub check: bool,
    /// A version the user chose to skip; they're asked again only for a newer one.
    pub skipped: Option<String>,
    /// Hidden until this time ("Not now").
    pub snoozed_until: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tools: Tools::default(),
            theme: "system".into(),
            updates: Updates::default(),
        }
    }
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            apps: true,
            disk: true,
        }
    }
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            check: true,
            skipped: None,
            snoozed_until: 0,
        }
    }
}

/// Settings plus where they live; shared by the API and the tray.
pub struct SettingsFile {
    path: PathBuf,
    pub current: Mutex<Settings>,
}

impl SettingsFile {
    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("settings.json");
        let current = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            path,
            current: Mutex::new(current),
        }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap().clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings> {
        let mut s = self.current.lock().unwrap();
        let mut next = s.clone();
        change(&mut next);
        write_atomic(&self.path, &serde_json::to_vec_pretty(&next)?)?;
        *s = next.clone();
        Ok(next)
    }
}

/// Writes then renames, so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
