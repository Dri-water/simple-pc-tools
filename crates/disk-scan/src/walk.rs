//! Fallback scanner: lists folders in parallel with plain std threads.
//!
//! Each worker takes a folder from a shared stack, lists it with
//! `read_dir` (on Windows the entry metadata comes from the listing itself,
//! no per-file calls), then appends the entries to the shared node list in
//! one go. A folder's node always exists before its entries are added, so
//! every parent index is smaller than its children's.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{check_cancel, leaf, set_phase, Node, NodeId, Progress};

type Skipped = Vec<(PathBuf, String)>;

struct Queue {
    pending: Vec<(NodeId, PathBuf)>,
    busy: usize,
    stop: bool,
}

struct Shared<'a> {
    queue: Mutex<Queue>,
    wake: Condvar,
    nodes: Mutex<Vec<Node>>,
    skipped: Mutex<Skipped>,
    progress: &'a Progress,
    cluster: u64,
}

pub fn walk(
    root: &Path,
    root_name: &str,
    progress: &Progress,
) -> anyhow::Result<(Vec<Node>, Skipped)> {
    set_phase(progress, "Listing folders");
    let cluster = cluster_size(root);
    let modified = fs::metadata(root)
        .ok()
        .map_or(0, |m| unix_time(m.modified().ok()));
    let shared = Shared {
        queue: Mutex::new(Queue {
            pending: vec![(0, root.to_path_buf())],
            busy: 0,
            stop: false,
        }),
        wake: Condvar::new(),
        nodes: Mutex::new(vec![leaf(root_name.into(), 0, true, 0, 0, modified)]),
        skipped: Mutex::new(Vec::new()),
        progress,
        cluster,
    };
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(4, 16);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| worker(&shared));
        }
    });
    check_cancel(progress)?;
    let nodes = shared.nodes.into_inner().unwrap_or_else(|e| e.into_inner());
    let mut skipped = shared
        .skipped
        .into_inner()
        .unwrap_or_else(|e| e.into_inner());
    skipped.sort();
    Ok((nodes, skipped))
}

fn worker(sh: &Shared) {
    let mut batch: Vec<Node> = Vec::new();
    let mut subdirs: Vec<(usize, PathBuf)> = Vec::new();
    loop {
        let (dir_id, dir) = {
            let mut q = sh.queue.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if q.stop {
                    return;
                }
                if let Some(job) = q.pending.pop() {
                    q.busy += 1;
                    break job;
                }
                if q.busy == 0 {
                    q.stop = true;
                    sh.wake.notify_all();
                    return;
                }
                q = sh.wake.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };

        if sh.progress.cancel.load(Ordering::Relaxed) {
            let mut q = sh.queue.lock().unwrap_or_else(|e| e.into_inner());
            q.stop = true;
            sh.wake.notify_all();
            return;
        }

        batch.clear();
        subdirs.clear();
        list_dir(sh, dir_id, &dir, &mut batch, &mut subdirs);

        let base = {
            let mut nodes = sh.nodes.lock().unwrap_or_else(|e| e.into_inner());
            let base = nodes.len();
            nodes.append(&mut batch);
            base
        };
        let mut q = sh.queue.lock().unwrap_or_else(|e| e.into_inner());
        for (i, path) in subdirs.drain(..) {
            q.pending.push(((base + i) as NodeId, path));
        }
        q.busy -= 1;
        sh.wake.notify_all();
    }
}

/// Lists one folder into `batch`; folders to descend into go to `subdirs`
/// as (index in `batch`, path).
fn list_dir(
    sh: &Shared,
    dir_id: NodeId,
    dir: &Path,
    batch: &mut Vec<Node>,
    subdirs: &mut Vec<(usize, PathBuf)>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            sh.skipped
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((dir.to_path_buf(), e.to_string()));
            return;
        }
    };
    let (mut files, mut bytes) = (0u64, 0u64);
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                sh.skipped
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((dir.to_path_buf(), e.to_string()));
                break;
            }
        };
        let Ok(meta) = entry.metadata() else { continue };
        let ft = meta.file_type();
        // Symlinks and junctions (including volume mount points) are listed
        // but never followed. Other reparse points (OneDrive folders,
        // dedup) are ordinary folders and are walked.
        let link = ft.is_symlink();
        let is_dir = meta.is_dir() || (link && is_dir_attr(&meta));
        let name: Box<str> = entry.file_name().to_string_lossy().into();
        let modified = unix_time(meta.modified().ok());
        if is_dir {
            if !link {
                subdirs.push((batch.len(), entry.path()));
            }
            batch.push(leaf(name, dir_id, true, 0, 0, modified));
        } else {
            let size = meta.len();
            let allocated = allocated_size(&meta, size, sh.cluster, || entry.path());
            files += 1;
            bytes += size;
            batch.push(leaf(name, dir_id, false, size, allocated, modified));
        }
    }
    sh.progress.files.fetch_add(files, Ordering::Relaxed);
    sh.progress.bytes.fetch_add(bytes, Ordering::Relaxed);
}

fn round_up(size: u64, cluster: u64) -> u64 {
    size.div_ceil(cluster) * cluster
}

#[cfg(windows)]
fn is_dir_attr(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x10 != 0 // FILE_ATTRIBUTE_DIRECTORY
}

#[cfg(not(windows))]
fn is_dir_attr(_meta: &fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
fn allocated_size(
    meta: &fs::Metadata,
    size: u64,
    cluster: u64,
    path: impl FnOnce() -> PathBuf,
) -> u64 {
    use std::os::windows::fs::MetadataExt;
    // SPARSE_FILE | COMPRESSED | OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS:
    // the listing's size says nothing about the space these use.
    const ODD: u32 = 0x200 | 0x800 | 0x1000 | 0x40000 | 0x400000;
    if meta.file_attributes() & ODD != 0 {
        if let Some(real) = crate::win::compressed_size(&path()) {
            return round_up(real, cluster);
        }
    }
    round_up(size, cluster)
}

#[cfg(not(windows))]
fn allocated_size(
    _meta: &fs::Metadata,
    size: u64,
    cluster: u64,
    _path: impl FnOnce() -> PathBuf,
) -> u64 {
    round_up(size, cluster)
}

#[cfg(windows)]
fn cluster_size(root: &Path) -> u64 {
    crate::win::volume_path(root)
        .ok()
        .and_then(|v| crate::win::cluster_size(&v))
        .unwrap_or(4096)
}

#[cfg(not(windows))]
fn cluster_size(_root: &Path) -> u64 {
    4096
}

pub(crate) fn unix_time(t: Option<SystemTime>) -> u64 {
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use crate::{scan, Method, Progress};
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::Ordering;

    fn write(path: &Path, len: usize) {
        fs::write(path, vec![7u8; len]).unwrap();
    }

    #[test]
    fn walk_rolls_up_sorts_and_finds() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        fs::create_dir_all(r.join("a/deep/deeper")).unwrap();
        fs::create_dir_all(r.join("b")).unwrap();
        fs::create_dir_all(r.join("empty")).unwrap();
        write(&r.join("top.bin"), 10);
        write(&r.join("a/one.bin"), 1000);
        write(&r.join("a/deep/two.bin"), 2000);
        write(&r.join("a/deep/deeper/three.bin"), 3000);
        write(&r.join("b/four.bin"), 500);

        let progress = Progress::default();
        let tree = scan(r, &progress).unwrap();
        assert_eq!(
            tree.method,
            Method::FolderWalk,
            "test shell is not elevated"
        );
        assert!(tree.skipped.is_empty(), "{:?}", tree.skipped);

        let root = &tree.nodes[tree.root as usize];
        assert_eq!(root.size, 6510);
        assert_eq!(root.files, 5);
        assert_eq!(root.dirs, 5);
        assert!(root.allocated >= root.size);
        assert_eq!(progress.files.load(Ordering::Relaxed), 5);
        assert_eq!(progress.bytes.load(Ordering::Relaxed), 6510);

        let names: Vec<&str> = tree
            .children(tree.root)
            .iter()
            .map(|&c| &*tree.nodes[c as usize].name)
            .collect();
        assert_eq!(names, ["a", "b", "top.bin", "empty"]);

        let a = tree.find(&r.join("A")).expect("case-insensitive find");
        assert_eq!(tree.nodes[a as usize].size, 6000);
        assert_eq!(tree.nodes[a as usize].files, 3);
        assert_eq!(tree.nodes[a as usize].dirs, 2);
        let a_kids: Vec<&str> = tree
            .children(a)
            .iter()
            .map(|&c| &*tree.nodes[c as usize].name)
            .collect();
        assert_eq!(a_kids, ["deep", "one.bin"]);

        // Every node's path finds the same node again.
        for id in 0..tree.nodes.len() as u32 {
            assert_eq!(tree.find(&tree.path(id)), Some(id), "{:?}", tree.path(id));
        }
        assert_eq!(tree.find(r), Some(tree.root));
        let with_slash = format!("{}\\", r.display());
        assert_eq!(tree.find(Path::new(&with_slash)), Some(tree.root));
        let three = tree.find(&r.join("a/deep/deeper/three.bin")).unwrap();
        assert_eq!(
            tree.path(three),
            r.join("a").join("deep").join("deeper").join("three.bin")
        );
        assert!(tree.children(three).is_empty());
        assert_eq!(tree.find(&r.join("missing")), None);
        assert_eq!(tree.find(Path::new(r"Z:\elsewhere")), None);
        assert_eq!(
            tree.nodes[tree.root as usize].name.as_ref(),
            crate::display_root(&r.to_string_lossy())
        );
    }

    #[test]
    fn walk_cancel_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("x")).unwrap();
        let progress = Progress::default();
        progress.cancel.store(true, Ordering::Relaxed);
        let err = scan(tmp.path(), &progress).unwrap_err();
        assert!(err.to_string().contains("cancel"), "{err}");

        // Cancel seen by the workers themselves (past the up-front check).
        let p = Progress::default();
        p.cancel.store(true, Ordering::Relaxed);
        let err = super::walk(tmp.path(), "root", &p).unwrap_err();
        assert!(err.to_string().contains("cancel"), "{err}");
    }

    #[test]
    fn walk_cancel_from_another_thread() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..200 {
            let d = tmp.path().join(format!("d{i}"));
            fs::create_dir(&d).unwrap();
            write(&d.join("f"), 1);
        }
        let p = Progress::default();
        std::thread::scope(|s| {
            s.spawn(|| {
                while p.files.load(Ordering::Relaxed) == 0 {
                    std::thread::yield_now();
                }
                p.cancel.store(true, Ordering::Relaxed);
            });
            // Either it finished before the cancel landed or it reports it.
            if let Err(e) = scan(tmp.path(), &p) {
                assert!(e.to_string().contains("cancel"));
            }
            p.cancel.store(true, Ordering::Relaxed);
        });
    }

    #[test]
    fn scan_rejects_files_and_missing_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("f.txt");
        write(&f, 3);
        let p = Progress::default();
        assert!(scan(&f, &p).is_err());
        assert!(scan(&tmp.path().join("nope"), &p).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn walk_does_not_follow_junctions() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        fs::create_dir(r.join("real")).unwrap();
        write(&r.join("real/big.bin"), 4000);
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(r.join("link"))
            .arg(r.join("real"))
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("mklink /J unavailable; skipping");
            return;
        }
        let tree = scan(r, &Progress::default()).unwrap();
        let root = &tree.nodes[tree.root as usize];
        assert_eq!(root.size, 4000);
        assert_eq!(root.files, 1);
        let link = tree.find(&r.join("link")).unwrap();
        assert!(tree.nodes[link as usize].is_dir);
        assert!(tree.children(link).is_empty());
    }
}
