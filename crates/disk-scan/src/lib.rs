//! Folder-size scanning for the Disk usage tool.
//!
//! `scan` builds an in-memory tree of every file and folder under a root, with
//! sizes rolled up into each folder. On a local NTFS volume, when the process
//! is elevated, it reads the volume's Master File Table directly (one long
//! sequential read, seconds for millions of files). Otherwise it walks the
//! folders in parallel.
//!
//! This file is the public interface the app depends on; keep it stable.
//!
//! Implementation notes:
//! - [`Tree`] keeps children in one flat array (`child_list[child_start[i]..
//!   child_start[i + 1]]`), so a file costs no allocation beyond its name.
//! - File table scans leave out the NTFS metadata files (records 0..24 other
//!   than the root, and everything under `$Extend`), named data streams and
//!   the space folders' own indexes take; a file with several hard links is
//!   counted once. Files whose parent folder record is gone (orphans) are left
//!   out.
//! - Folder walks count every hard link separately and round sizes up to the
//!   cluster size (sparse, compressed and cloud placeholder files are asked
//!   for their real on-disk size). Symlinks and junctions are listed but not
//!   followed, by both methods.

mod mft;
#[cfg(windows)]
mod volume;
mod walk;
#[cfg(windows)]
mod win;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// Index of a node in [`Tree::nodes`].
pub type NodeId = u32;

#[derive(Debug, Clone)]
pub struct Node {
    /// File or folder name (the root's name is its full path, e.g. `C:\`).
    pub name: Box<str>,
    /// Parent node; the root is its own parent.
    pub parent: NodeId,
    pub is_dir: bool,
    /// Logical size in bytes; for folders, the sum of everything under it.
    pub size: u64,
    /// Space used on disk in bytes (cluster-rounded, smaller when
    /// compressed or sparse); for folders, the sum of everything under it.
    pub allocated: u64,
    /// Files under this folder, recursively (1 for a file).
    pub files: u64,
    /// Folders under this folder, recursively (0 for a file).
    pub dirs: u64,
    /// Last write time, seconds since the Unix epoch (0 if unknown).
    pub modified: u64,
}

/// How a scan read the disk; shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Read the NTFS Master File Table directly.
    FileTable,
    /// Listed folders one by one.
    FolderWalk,
}

#[derive(Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub root: NodeId,
    pub method: Method,
    /// Folders that could not be read (access denied etc.), with the reason.
    pub skipped: Vec<(PathBuf, String)>,
    // Implementation detail: children of each node, sorted by size, largest
    // first: `child_list[child_start[id]..child_start[id + 1]]`.
    child_start: Vec<u32>,
    child_list: Vec<NodeId>,
}

/// Live progress of a running scan, shared with the UI thread.
#[derive(Debug, Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    /// Set to stop the scan early; `scan` then returns an error.
    pub cancel: AtomicBool,
    /// Short human description of what is happening ("Reading file table").
    pub phase: Mutex<String>,
}

impl Tree {
    /// Children of `id`, largest first. Empty for files.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        let i = id as usize;
        match (self.child_start.get(i), self.child_start.get(i + 1)) {
            (Some(&a), Some(&b)) => &self.child_list[a as usize..b as usize],
            _ => &[],
        }
    }

    /// Full path of a node.
    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut chain = Vec::new();
        let mut cur = id;
        while cur != self.root {
            let Some(node) = self.nodes.get(cur as usize) else {
                break;
            };
            chain.push(cur);
            if node.parent == cur {
                break;
            }
            cur = node.parent;
        }
        let mut path = PathBuf::from(&*self.nodes[self.root as usize].name);
        for &n in chain.iter().rev() {
            path.push(&*self.nodes[n as usize].name);
        }
        path
    }

    /// The node at `path`, if it is inside the scanned root (case-insensitive).
    pub fn find(&self, path: &Path) -> Option<NodeId> {
        let want = path_components(&path.to_string_lossy());
        let root = path_components(&self.nodes.get(self.root as usize)?.name);
        if want.len() < root.len() || want[..root.len()] != root[..] {
            return None;
        }
        let mut id = self.root;
        for comp in &want[root.len()..] {
            id = *self
                .children(id)
                .iter()
                .find(|&&c| eq_lower(&self.nodes[c as usize].name, comp))?;
        }
        Some(id)
    }
}

/// Scans everything under `root` (a drive like `C:\` or any folder).
/// Tries the file table first when it can be used, then falls back to walking.
pub fn scan(root: &Path, progress: &Progress) -> anyhow::Result<Tree> {
    let abs = std::path::absolute(root)?;
    let root_name = display_root(&abs.to_string_lossy());
    let meta =
        std::fs::metadata(&abs).map_err(|e| anyhow::anyhow!("Cannot open {root_name}: {e}"))?;
    if !meta.is_dir() {
        anyhow::bail!("{root_name} is not a folder");
    }
    check_cancel(progress)?;

    #[cfg(windows)]
    if let Some(target) = volume::file_table_target(&abs) {
        set_phase(progress, "Reading file table");
        match volume::scan_file_table(&target, &root_name, progress) {
            Ok(nodes) => return Ok(finish(nodes, Method::FileTable, Vec::new(), progress)),
            Err(e) => {
                check_cancel(progress)?;
                set_phase(
                    progress,
                    &format!("File table unreadable ({e}); listing folders"),
                );
                progress.files.store(0, Ordering::Relaxed);
                progress.bytes.store(0, Ordering::Relaxed);
            }
        }
    }

    let (nodes, skipped) = walk::walk(&abs, &root_name, progress)?;
    Ok(finish(nodes, Method::FolderWalk, skipped, progress))
}

/// Error returned when `progress.cancel` was set.
fn check_cancel(progress: &Progress) -> anyhow::Result<()> {
    if progress.cancel.load(Ordering::Relaxed) {
        anyhow::bail!("Scan cancelled");
    }
    Ok(())
}

fn set_phase(progress: &Progress, phase: &str) {
    if let Ok(mut p) = progress.phase.lock() {
        phase.clone_into(&mut p);
    }
}

/// A node as the scanners produce it: its own size only, no roll-up yet.
fn leaf(
    name: Box<str>,
    parent: NodeId,
    is_dir: bool,
    size: u64,
    allocated: u64,
    modified: u64,
) -> Node {
    Node {
        name,
        parent,
        is_dir,
        size: if is_dir { 0 } else { size },
        allocated: if is_dir { 0 } else { allocated },
        files: u64::from(!is_dir),
        dirs: 0,
        modified,
    }
}

/// Turns scanner output into a [`Tree`]. `nodes[0]` is the root and every
/// other node's parent has a smaller index, so one reverse pass rolls sizes up
/// (every descendant of a node has a larger index than the node).
fn finish(
    mut nodes: Vec<Node>,
    method: Method,
    skipped: Vec<(PathBuf, String)>,
    progress: &Progress,
) -> Tree {
    set_phase(progress, "Adding up sizes");
    for i in (1..nodes.len()).rev() {
        let n = &nodes[i];
        let (p, size, allocated, files, dirs) = (
            n.parent as usize,
            n.size,
            n.allocated,
            n.files,
            n.dirs + u64::from(n.is_dir),
        );
        debug_assert!(p < i, "parent must come before child");
        let parent = &mut nodes[p];
        parent.size += size;
        parent.allocated += allocated;
        parent.files += files;
        parent.dirs += dirs;
    }

    let mut child_start = vec![0u32; nodes.len() + 1];
    for n in nodes.iter().skip(1) {
        child_start[n.parent as usize + 1] += 1;
    }
    for i in 1..child_start.len() {
        child_start[i] += child_start[i - 1];
    }
    let mut fill = child_start.clone();
    let mut child_list = vec![0 as NodeId; nodes.len().saturating_sub(1)];
    for (i, n) in nodes.iter().enumerate().skip(1) {
        let slot = &mut fill[n.parent as usize];
        child_list[*slot as usize] = i as NodeId;
        *slot += 1;
    }
    drop(fill);
    for w in child_start.windows(2) {
        child_list[w[0] as usize..w[1] as usize].sort_unstable_by(|&a, &b| {
            let (a, b) = (&nodes[a as usize], &nodes[b as usize]);
            b.size
                .cmp(&a.size)
                .then_with(|| b.allocated.cmp(&a.allocated))
                .then_with(|| a.name.cmp(&b.name))
        });
    }

    if let Some(root) = nodes.first() {
        progress.files.store(root.files, Ordering::Relaxed);
        progress.bytes.store(root.size, Ordering::Relaxed);
    }
    set_phase(progress, "Done");
    Tree {
        nodes,
        root: 0,
        method,
        skipped,
        child_start,
        child_list,
    }
}

/// `C:\Users\x` style name for the scan root: no `\\?\` prefix, backslashes,
/// no trailing separator except on a drive root (`C:\`).
fn display_root(s: &str) -> String {
    let s = s.replace('/', "\\");
    let s = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    };
    let trimmed = s.trim_end_matches('\\');
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        format!("{trimmed}\\")
    } else if trimmed.is_empty() {
        s
    } else {
        trimmed.to_string()
    }
}

/// Lower-cased path components for comparisons, with `.` and `..` resolved.
fn path_components(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in display_root(s).split('\\') {
        match c {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            c => out.push(c.to_lowercase()),
        }
    }
    out
}

/// `name` equals `lower` (already lower-cased), ignoring case.
fn eq_lower(name: &str, lower: &str) -> bool {
    if name.is_ascii() && lower.is_ascii() {
        name.eq_ignore_ascii_case(lower)
    } else {
        name.chars().flat_map(char::to_lowercase).eq(lower.chars())
    }
}

/// Local drives to offer for scanning: (root like `C:\`, label, total bytes, free bytes).
pub fn drives() -> Vec<Drive> {
    #[cfg(windows)]
    {
        win::drives()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[derive(Debug, Clone)]
pub struct Drive {
    pub root: PathBuf,
    pub label: String,
    pub file_system: String,
    pub total: u64,
    pub free: u64,
}

/// Whether this process runs elevated (the file table needs it).
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        win::is_elevated()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_root_normalises() {
        assert_eq!(display_root(r"\\?\C:\"), r"C:\");
        assert_eq!(display_root("C:"), r"C:\");
        assert_eq!(display_root(r"C:\Users\x\"), r"C:\Users\x");
        assert_eq!(display_root("C:/Users/x"), r"C:\Users\x");
        assert_eq!(display_root(r"\\?\UNC\srv\share"), r"\\srv\share");
    }

    #[test]
    fn components_ignore_case_and_dots() {
        assert_eq!(
            path_components(r"c:\Users\.\X\..\Y\"),
            vec!["c:".to_string(), "users".into(), "y".into()]
        );
        assert!(eq_lower("ÄBC", "äbc"));
        assert!(!eq_lower("abc", "abd"));
    }
}
