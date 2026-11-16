//! Scans a folder or drive and prints a short summary.
//!
//! `cargo run --release --example scan -- C:\`
//! Run it from an elevated prompt to use the NTFS file table.

use std::path::PathBuf;
use std::time::Instant;

fn human(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < units.len() {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", units[u])
}

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    println!("elevated: {}", disk_scan::is_elevated());
    let progress = disk_scan::Progress::default();
    let t0 = Instant::now();
    let tree = disk_scan::scan(&root, &progress)?;
    let elapsed = t0.elapsed();
    let r = &tree.nodes[tree.root as usize];
    println!("method:   {:?}", tree.method);
    println!("root:     {}", r.name);
    println!("files:    {}  folders: {}", r.files, r.dirs);
    println!(
        "size:     {}  on disk: {}",
        human(r.size),
        human(r.allocated)
    );
    println!("elapsed:  {elapsed:.2?}");
    println!("skipped:  {} folders", tree.skipped.len());
    println!();
    for &c in tree.children(tree.root).iter().take(10) {
        let n = &tree.nodes[c as usize];
        println!(
            "{:>10}  {:>9} files  {}{}",
            human(n.size),
            n.files,
            n.name,
            if n.is_dir { "\\" } else { "" }
        );
    }
    Ok(())
}
