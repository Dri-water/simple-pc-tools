//! Update check: once a day, asks GitHub for the latest release. The UI offers
//! Update / Not now / Skip this version, and Settings can turn checking off
//! for good. Updating downloads the release's installer, checks its SHA-256
//! against the one GitHub publishes, and runs it; the installer closes this
//! copy, replaces the files and starts the new one.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::settings::{now, SettingsFile};

pub const REPO: &str = "Dri-water/simple-pc-tools";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const DAY: u64 = 24 * 3600;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Release {
    pub version: String,
    pub page: String,
    pub notes: String,
    pub installer_url: Option<String>,
    pub installer_size: u64,
    /// "sha256:<hex>" as published by GitHub.
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateState {
    pub current: String,
    pub latest: Option<Release>,
    pub last_checked: u64,
    pub checking: bool,
    pub error: Option<String>,
    /// Download progress, 0..=1, while updating.
    pub downloading: Option<f32>,
}

pub struct Updater {
    pub state: Mutex<UpdateState>,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

impl Updater {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(UpdateState {
                current: VERSION.into(),
                ..Default::default()
            }),
            http: reqwest::Client::builder()
                // GitHub requires a User-Agent; this names the app, nothing about the user.
                .user_agent(concat!("simple-pc-tools/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(60))
                .build()
                .expect("http client"),
        })
    }

    /// Checks shortly after start and then daily, while checking is on.
    pub async fn run(self: Arc<Self>, settings: Arc<SettingsFile>) {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            if settings.get().updates.check
                && now() - self.state.lock().unwrap().last_checked >= DAY
            {
                let _ = self.check().await;
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    }

    pub async fn check(&self) -> Result<()> {
        self.state.lock().unwrap().checking = true;
        let result = self.fetch_latest().await;
        let mut s = self.state.lock().unwrap();
        s.checking = false;
        s.last_checked = now();
        match result {
            Ok(r) => {
                s.latest = r;
                s.error = None;
                Ok(())
            }
            Err(e) => {
                s.error = Some(format!("Couldn't reach GitHub: {e:#}"));
                Err(e)
            }
        }
    }

    async fn fetch_latest(&self) -> Result<Option<Release>> {
        let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
        let resp = self
            .http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None); // no releases yet
        }
        let gh: GhRelease = resp.error_for_status()?.json().await?;
        Ok(release_from(gh))
    }

    /// Downloads the latest installer, verifies it and runs it.
    pub async fn install(&self) -> Result<()> {
        let release = self
            .state
            .lock()
            .unwrap()
            .latest
            .clone()
            .context("no update found yet")?;
        let url = release
            .installer_url
            .clone()
            .context("this release has no installer")?;
        self.state.lock().unwrap().downloading = Some(0.0);
        let result = self.download_and_run(&release, &url).await;
        self.state.lock().unwrap().downloading = None;
        result
    }

    async fn download_and_run(&self, release: &Release, url: &str) -> Result<()> {
        let resp = self.http.get(url).send().await?.error_for_status()?;
        let total = resp
            .content_length()
            .unwrap_or(release.installer_size)
            .max(1);
        let mut hasher = Sha256::new();
        let mut bytes = Vec::with_capacity(total as usize);
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            hasher.update(&chunk);
            bytes.extend_from_slice(&chunk);
            self.state.lock().unwrap().downloading = Some(bytes.len() as f32 / total as f32);
        }
        let got = format!("sha256:{:x}", hasher.finalize());
        match &release.digest {
            Some(want) if !want.eq_ignore_ascii_case(&got) => {
                bail!("the download didn't match the published checksum, so it wasn't run")
            }
            _ => {}
        }
        let dir = std::env::temp_dir().join("simple-pc-tools-update");
        std::fs::create_dir_all(&dir)?;
        let file = dir.join(format!("simple-pc-tools-setup-{}.exe", release.version));
        std::fs::write(&file, &bytes)?;
        // The installer closes this copy, installs, and starts the new version.
        // It must outlive us, so start it outside our job object when allowed.
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let mut cmd = std::process::Command::new(&file);
        cmd.args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]);
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        if cmd.spawn().is_err() {
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
            cmd.spawn().context("starting the installer")?;
        }
        Ok(())
    }
}

/// Is `latest` newer than the running version?
pub fn is_newer(latest: &str) -> bool {
    match (
        semver::Version::parse(latest),
        semver::Version::parse(VERSION),
    ) {
        (Ok(l), Ok(c)) => l > c,
        _ => false,
    }
}

fn release_from(gh: GhRelease) -> Option<Release> {
    if gh.draft || gh.prerelease {
        return None;
    }
    let version = gh.tag_name.trim_start_matches('v').to_string();
    semver::Version::parse(&version).ok()?;
    let installer = gh
        .assets
        .into_iter()
        .find(|a| a.name.starts_with("simple-pc-tools-setup") && a.name.ends_with(".exe"));
    Some(Release {
        version,
        page: gh.html_url,
        notes: gh.body.chars().take(4000).collect(),
        installer_size: installer.as_ref().map_or(0, |a| a.size),
        digest: installer.as_ref().and_then(|a| a.digest.clone()),
        installer_url: installer.map(|a| a.browser_download_url),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_github_release() {
        let gh: GhRelease = serde_json::from_value(serde_json::json!({
            "tag_name": "v9.1.0",
            "html_url": "https://github.com/x/y/releases/tag/v9.1.0",
            "body": "Fixes",
            "assets": [
                { "name": "simple-pc-tools-9.1.0-portable.zip", "browser_download_url": "z", "size": 1 },
                { "name": "simple-pc-tools-setup-9.1.0.exe", "browser_download_url": "u", "size": 5,
                  "digest": "sha256:ab" }
            ]
        }))
        .unwrap();
        let r = release_from(gh).unwrap();
        assert_eq!(r.version, "9.1.0");
        assert_eq!(r.installer_url.as_deref(), Some("u"));
        assert_eq!(r.digest.as_deref(), Some("sha256:ab"));
        assert!(is_newer(&r.version));
        assert!(!is_newer(VERSION));
    }
}
