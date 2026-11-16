//! Fast scanner: reads an NTFS volume's Master File Table directly.
//!
//! Boot sector -> location of `$MFT` and the record size; record 0 (`$MFT`
//! itself) -> the data runs of the whole table (following its
//! `$ATTRIBUTE_LIST` when the table is fragmented enough to need one); then
//! the table is read front to back in large chunks on a reader thread while
//! this thread parses the records.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use anyhow::{bail, Context};

use crate::mft::{self, Collector, Parsed, Run};
use crate::win::{self, Handle};
use crate::{check_cancel, set_phase, Node, Progress};

/// Logical bytes of the table read per chunk.
const CHUNK: u64 = 4 << 20;

pub struct Target {
    /// `\\?\Volume{GUID}` (no trailing backslash), openable for raw reads.
    device: String,
    /// MFT record of the scan root (5 for the volume root).
    top: u64,
}

/// Where the file table can be used for `abs`: elevated, local NTFS, and
/// the folder really lives on that volume (not behind a junction elsewhere).
pub fn file_table_target(abs: &Path) -> Option<Target> {
    if !win::is_elevated() {
        return None;
    }
    let mount = win::volume_path(abs).ok()?;
    if !win::is_local(&mount) {
        return None;
    }
    let info = win::volume_info(&mount).ok()?;
    if !info.file_system.eq_ignore_ascii_case("NTFS") {
        return None;
    }
    let (serial, frn) = win::file_id(abs).ok()?;
    if serial != info.serial {
        return None;
    }
    let guid = win::volume_guid_path(&mount).ok()?;
    Some(Target {
        device: guid.trim_end_matches('\\').to_string(),
        top: frn & 0x0000_FFFF_FFFF_FFFF,
    })
}

/// Volume geometry from the NTFS boot sector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Geometry {
    pub sector: u64,
    pub cluster: u64,
    pub mft_lcn: u64,
    pub record: u64,
}

pub(crate) fn parse_boot(b: &[u8]) -> anyhow::Result<Geometry> {
    if b.get(3..11) != Some(b"NTFS    ") {
        bail!("not an NTFS boot sector");
    }
    let sector = u64::from(mft::u16_at(b, 0x0B).unwrap_or(0));
    let spc = b[0x0D];
    let spc = if spc > 0x80 {
        1u64 << (256 - u32::from(spc))
    } else {
        u64::from(spc)
    };
    let cluster = sector * spc;
    let mft_lcn = mft::u64_at(b, 0x30).unwrap_or(0);
    let cpr = b[0x40] as i8;
    let record = if cpr < 0 {
        1u64 << (-i32::from(cpr))
    } else {
        cpr as u64 * cluster
    };
    if !sector.is_power_of_two() || sector < 512 || cluster == 0 || !(512..=65536).contains(&record)
    {
        bail!("unexpected NTFS geometry (sector {sector}, cluster {cluster}, record {record})");
    }
    Ok(Geometry {
        sector,
        cluster,
        mft_lcn,
        record,
    })
}

/// The table's layout: extents as (first VCN, run).
struct Layout {
    extents: Vec<(u64, Run)>,
    size: u64,
}

impl Layout {
    fn from_runs(mut parts: Vec<(u64, Vec<Run>)>, size: u64) -> Layout {
        parts.sort_by_key(|p| p.0);
        let mut extents = Vec::new();
        for (start, runs) in parts {
            let mut vcn = start;
            for r in runs {
                extents.push((vcn, r));
                vcn += r.len;
            }
        }
        Layout { extents, size }
    }
}

/// Page-aligned scratch buffer (raw volume reads want aligned memory).
struct Aligned {
    v: Vec<u8>,
    pad: usize,
}

impl Aligned {
    fn new(len: usize) -> Aligned {
        let v = vec![0u8; len + 4096];
        let pad = v.as_ptr().align_offset(4096).min(4096);
        Aligned { v, pad }
    }
    fn get(&mut self, len: usize) -> &mut [u8] {
        &mut self.v[self.pad..self.pad + len]
    }
}

struct Reader<'a> {
    vol: &'a Handle,
    geo: Geometry,
}

impl Reader<'_> {
    /// Reads `len` bytes of the stream described by `layout` at logical
    /// `offset` (a cluster multiple) into `out`, which must have room for
    /// `len` rounded up to a cluster.
    fn read_logical(
        &self,
        layout: &Layout,
        offset: u64,
        len: u64,
        out: &mut [u8],
    ) -> anyhow::Result<()> {
        let c = self.geo.cluster;
        let mut done = 0u64;
        for &(vcn, run) in &layout.extents {
            if done >= len {
                break;
            }
            let (run_start, run_end) = (vcn * c, (vcn + run.len) * c);
            let pos = offset + done;
            if run_end <= pos {
                continue;
            }
            if run_start > pos {
                bail!("gap in the file table's data runs");
            }
            let take = (run_end - pos).min(len - done);
            let read_len = take.div_ceil(c) * c;
            let dst = &mut out[done as usize..(done + read_len) as usize];
            match run.lcn {
                Some(lcn) => self
                    .vol
                    .read_at(lcn * c + (pos - run_start), dst)
                    .context("reading the file table")?,
                None => dst.fill(0),
            }
            done += take;
        }
        if done < len {
            bail!("file table data runs end early");
        }
        Ok(())
    }

    /// Reads and fixes up record `no` using what is known of the layout.
    fn record(&self, layout: &Layout, no: u64) -> anyhow::Result<Vec<u8>> {
        let rs = self.geo.record;
        let c = self.geo.cluster;
        let off = no * rs;
        let start = off / c * c;
        let len = (off + rs).div_ceil(c) * c - start;
        let mut buf = Aligned::new(len as usize);
        self.read_logical(layout, start, len, buf.get(len as usize))?;
        let skip = (off - start) as usize;
        Ok(buf.get(len as usize)[skip..skip + rs as usize].to_vec())
    }
}

/// Data runs of `$MFT` from its record 0 (and its extension records).
fn mft_layout(rd: &Reader) -> anyhow::Result<Layout> {
    let g = rd.geo;
    let len = g.record.div_ceil(g.sector) * g.sector;
    let mut buf = Aligned::new(len as usize);
    rd.vol
        .read_at(g.mft_lcn * g.cluster, buf.get(len as usize))
        .context("reading $MFT record 0")?;
    let mut rec0 = buf.get(len as usize)[..g.record as usize].to_vec();
    if !mft::apply_fixups(&mut rec0) {
        bail!("$MFT record 0 is damaged");
    }

    let mut parts = Vec::new();
    let mut size = None;
    let mut list = None;
    for a in mft::attributes(&rec0) {
        if a.kind == mft::ATTR_DATA && a.name_len == 0 {
            let (vcn, sz, runs) = mft::non_resident_runs(&rec0, &a).context("$MFT data runs")?;
            if vcn == 0 {
                size = Some(sz);
            }
            parts.push((vcn, runs));
        } else if a.kind == mft::ATTR_ATTRIBUTE_LIST {
            list = Some(a);
        }
    }
    let size = size.context("$MFT has no data attribute")?;
    let mut layout = Layout::from_runs(parts.clone(), size);

    // A fragmented table keeps further extents of its $DATA in extension
    // records named by the attribute list.
    if let Some(a) = list {
        let bytes = match a.value(&rec0) {
            Some((off, l)) => rec0[off..off + l].to_vec(),
            None => {
                let (_, sz, runs) =
                    mft::non_resident_runs(&rec0, &a).context("attribute list runs")?;
                let list_layout = Layout::from_runs(vec![(0, runs)], sz);
                let padded = sz.div_ceil(g.cluster) * g.cluster;
                let mut b = Aligned::new(padded as usize);
                rd.read_logical(&list_layout, 0, sz, b.get(padded as usize))?;
                b.get(sz as usize).to_vec()
            }
        };
        let mut ext_records = Vec::new();
        let mut o = 0usize;
        while o + 0x1A <= bytes.len() {
            let kind = mft::u32_at(&bytes, o).unwrap_or(0);
            let len = mft::u16_at(&bytes, o + 4).unwrap_or(0) as usize;
            let name_len = bytes[o + 6];
            let rec = mft::u64_at(&bytes, o + 0x10).unwrap_or(0) & 0x0000_FFFF_FFFF_FFFF;
            if len == 0 {
                break;
            }
            if kind == mft::ATTR_DATA && name_len == 0 && rec != 0 && !ext_records.contains(&rec) {
                ext_records.push(rec);
            }
            o += len;
        }
        for rec in ext_records {
            let mut r = rd.record(&layout, rec)?;
            if !mft::apply_fixups(&mut r) {
                bail!("$MFT extension record {rec} is damaged");
            }
            for a in mft::attributes(&r) {
                if a.kind == mft::ATTR_DATA && a.name_len == 0 {
                    if let Some((vcn, _, runs)) = mft::non_resident_runs(&r, &a) {
                        parts.push((vcn, runs));
                    }
                }
            }
            layout = Layout::from_runs(parts.clone(), size);
        }
    }
    Ok(layout)
}

pub fn scan_file_table(
    t: &Target,
    root_name: &str,
    progress: &Progress,
) -> anyhow::Result<Vec<Node>> {
    let vol = Handle::open_volume(&t.device).with_context(|| format!("opening {}", t.device))?;
    let mut boot = Aligned::new(4096);
    vol.read_at(0, boot.get(4096))
        .context("reading the boot sector")?;
    let geo = parse_boot(boot.get(4096))?;
    let rd = Reader { vol: &vol, geo };
    let layout = mft_layout(&rd)?;
    let records = layout.size / geo.record;
    if records > u64::from(u32::MAX) {
        bail!("file table too large");
    }

    let chunk = (CHUNK.div_ceil(geo.cluster) * geo.cluster)
        .max(geo.record.div_ceil(geo.cluster) * geo.cluster);
    let buf_len = (chunk + geo.cluster) as usize;
    let mut collector = Collector::new(records as usize);
    let mut parsed = Parsed::default();

    std::thread::scope(|s| -> anyhow::Result<()> {
        let (full_tx, full_rx) = mpsc::sync_channel::<anyhow::Result<(u64, u64, Aligned)>>(2);
        let (empty_tx, empty_rx) = mpsc::channel::<Aligned>();
        for _ in 0..3 {
            let _ = empty_tx.send(Aligned::new(buf_len));
        }
        let rd = &rd;
        let layout = &layout;
        s.spawn(move || {
            let mut off = 0u64;
            while off < layout.size {
                let Ok(mut buf) = empty_rx.recv() else { return };
                let len = chunk.min(layout.size - off);
                let res = rd.read_logical(layout, off, len, buf.get(buf_len));
                let msg = res.map(|()| (off, len, buf));
                let failed = msg.is_err();
                if full_tx.send(msg).is_err() || failed {
                    return;
                }
                off += len;
            }
        });

        let rs = geo.record as usize;
        for msg in full_rx.iter() {
            let (off, len, mut buf) = msg?;
            check_cancel(progress)?;
            let first = off / geo.record;
            for (k, rec) in buf.get(len as usize).chunks_exact_mut(rs).enumerate() {
                let no = first + k as u64;
                if mft::parse_record(rec, &mut parsed) {
                    collector.add(no, rec, &parsed);
                }
            }
            progress.files.store(collector.files, Ordering::Relaxed);
            progress.bytes.store(collector.bytes, Ordering::Relaxed);
            let _ = empty_tx.send(buf);
        }
        Ok(())
    })?;

    check_cancel(progress)?;
    set_phase(progress, "Building tree");
    collector
        .build(t.top, root_name)
        .context("scan folder not found in the file table")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_sector_geometry() {
        let mut b = vec![0u8; 512];
        b[3..11].copy_from_slice(b"NTFS    ");
        b[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes());
        b[0x0D] = 8;
        b[0x30..0x38].copy_from_slice(&786_432u64.to_le_bytes());
        b[0x40] = 0xF6; // -10: 1024-byte records
        assert_eq!(
            parse_boot(&b).unwrap(),
            Geometry {
                sector: 512,
                cluster: 4096,
                mft_lcn: 786_432,
                record: 1024
            }
        );
        b[0x0D] = 0xF4; // 2^12 sectors per cluster (large-cluster encoding)
        assert_eq!(parse_boot(&b).unwrap().cluster, 512 << 12);
        b[3] = b'X';
        assert!(parse_boot(&b).is_err());
    }

    /// Reads the real C: file table. Needs an elevated shell:
    /// `cargo test -- --ignored real_volume` from an administrator prompt.
    #[test]
    #[ignore = "needs an elevated shell"]
    fn real_volume_file_table() {
        assert!(win::is_elevated(), "run this test from an elevated shell");
        let progress = Progress::default();
        let t0 = std::time::Instant::now();
        let tree = crate::scan(Path::new(r"C:\"), &progress).unwrap();
        assert_eq!(tree.method, crate::Method::FileTable);
        let root = &tree.nodes[tree.root as usize];
        eprintln!(
            "{} files, {} bytes in {:?}",
            root.files,
            root.size,
            t0.elapsed()
        );
        assert!(root.files > 10_000);
        let win_dir = tree
            .find(Path::new(r"C:\Windows\System32"))
            .expect("System32");
        assert!(tree.nodes[win_dir as usize].size > 1 << 30);

        // A subfolder scan is re-rooted and agrees with the full scan.
        let sub = crate::scan(Path::new(r"C:\Windows"), &Progress::default()).unwrap();
        assert_eq!(&*sub.nodes[sub.root as usize].name, r"C:\Windows");
        let full = tree.find(Path::new(r"C:\Windows")).unwrap();
        assert_eq!(
            sub.nodes[sub.root as usize].files,
            tree.nodes[full as usize].files
        );
    }
}
