//! Copies started from inside a packaged (MSIX) app's sandbox.
//!
//! Some packaged apps (Claude Desktop, and some Microsoft Store apps) run the
//! programs they start in their own container. Windows then quietly redirects
//! that program's writes under AppData into the package's private folder,
//! `%LOCALAPPDATA%\Packages\<package>\LocalCache\Local\...`, and shows it the
//! redirected files as if they were in AppData. Every process started from it
//! inherits this, including an installer and the elevated copy it opens.
//!
//! A copy of Simple PC Tools running like that saves its app list and
//! settings where the copy that starts with Windows (from Task Scheduler,
//! outside any container) never looks: after a restart, the apps are gone.
//!
//! So: [`redirected`] tells a copy it's in such a sandbox, so it can hand over
//! to the sign-in task instead of running there; [`stranded`] finds app lists
//! that copies already left behind in a package folder, so they can be
//! brought back.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// The package copies of `dir` (`Packages\*\LocalCache\Local\<dir under
/// LOCALAPPDATA>`), or nothing if `dir` isn't under LOCALAPPDATA.
fn package_copies(local: &Path, dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Ok(rel) = dir.strip_prefix(local) else {
        return Vec::new();
    };
    let Ok(packages) = std::fs::read_dir(local.join("Packages")) else {
        return Vec::new();
    };
    packages
        .flatten()
        .map(|p| {
            let package = p.path();
            let copy = package.join("LocalCache").join("Local").join(rel);
            (package, copy)
        })
        .collect()
}

/// If this process's writes to `dir` are being redirected into a package's
/// folder, returns that package's folder. Finds out by writing a small file
/// and looking for where it landed.
pub fn redirected(dir: &Path) -> Option<PathBuf> {
    redirected_in(&local_app_data()?, dir)
}

fn redirected_in(local: &Path, dir: &Path) -> Option<PathBuf> {
    if !dir.starts_with(local) || std::fs::create_dir_all(dir).is_err() {
        return None;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!(".where-am-i-{}-{nanos}", std::process::id());
    let probe = dir.join(&name);
    std::fs::write(&probe, b"").ok()?;
    let found = package_copies(local, dir)
        .into_iter()
        .find(|(_, copy)| copy.join(&name).exists())
        .map(|(package, _)| package);
    let _ = std::fs::remove_file(&probe);
    found
}

/// Folders inside packages that hold a copy of `dir` with a file named
/// `file`, newest file first.
pub fn stranded(dir: &Path, file: &str) -> Vec<PathBuf> {
    local_app_data().map_or_else(Vec::new, |l| stranded_in(&l, dir, file))
}

fn stranded_in(local: &Path, dir: &Path, file: &str) -> Vec<PathBuf> {
    let mut found: Vec<(SystemTime, PathBuf)> = package_copies(local, dir)
        .into_iter()
        .filter_map(|(_, copy)| {
            let modified = std::fs::metadata(copy.join(file)).ok()?.modified().ok()?;
            Some((modified, copy))
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|f| f.1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("spt-sandbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn not_redirected_when_the_file_lands_where_written() {
        let local = temp("plain");
        std::fs::create_dir_all(local.join("Packages/Some.App_123/LocalCache/Local")).unwrap();
        assert_eq!(redirected_in(&local, &local.join("Simple PC Tools")), None);
        // The probe file is cleaned up.
        assert_eq!(
            std::fs::read_dir(local.join("Simple PC Tools"))
                .unwrap()
                .count(),
            0
        );
        // Folders outside LOCALAPPDATA are never redirected.
        assert_eq!(redirected_in(&local, &std::env::temp_dir()), None);
        let _ = std::fs::remove_dir_all(&local);
    }

    /// Manual check: run from a terminal inside a packaged app (for example
    /// Claude Desktop) with `cargo test -- --ignored sandbox::tests::live`.
    #[test]
    #[ignore]
    fn live() {
        let dir = local_app_data().unwrap().join("spt-sandbox-check");
        println!("redirected into: {:?}", redirected(&dir));
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn finds_stranded_copies_newest_first() {
        let local = temp("stranded");
        let dir = local.join("Simple PC Tools");
        let a = local.join("Packages/A_1/LocalCache/Local/Simple PC Tools");
        let b = local.join("Packages/B_2/LocalCache/Local/Simple PC Tools");
        let c = local.join("Packages/C_3/LocalCache/Local/Simple PC Tools");
        for d in [&a, &b, &c] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(a.join("apps.json"), "{}").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(b.join("apps.json"), "{}").unwrap();
        // c has no apps.json.
        assert_eq!(stranded_in(&local, &dir, "apps.json"), vec![b, a]);
        assert!(stranded_in(&local, &std::env::temp_dir(), "apps.json").is_empty());
        let _ = std::fs::remove_dir_all(&local);
    }
}
