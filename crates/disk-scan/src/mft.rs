//! NTFS Master File Table record parsing, independent of how the bytes were
//! read (so it can be tested on synthetic records).
//!
//! A FILE record is a header, an update-sequence array and a list of
//! attributes. We read:
//! - 0x10 `$STANDARD_INFORMATION`: last write time,
//! - 0x30 `$FILE_NAME`: name and parent folder (one per hard link, plus a
//!   separate DOS 8.3 name for long names),
//! - 0x80 `$DATA` (unnamed stream only): logical and allocated size.
//!
//! Files with many attributes spill into extension records whose header
//! points at the base record; `$ATTRIBUTE_LIST` (0x20) only says which
//! extension holds what, so instead of reading it we fold every extension
//! record into its base by that back-pointer.

#![cfg_attr(not(windows), allow(dead_code))]

use crate::{leaf, Node, NodeId};

pub const ROOT_RECORD: u64 = 5;
/// Records below this are NTFS metadata ($MFT, $LogFile, $Bitmap, ...),
/// except the root folder (5). They are left out of the tree.
pub const FIRST_USER_RECORD: u64 = 24;
const FIXUP_STRIDE: usize = 512;
const REF_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

pub const ATTR_STANDARD_INFORMATION: u32 = 0x10;
pub const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
pub const ATTR_FILE_NAME: u32 = 0x30;
pub const ATTR_DATA: u32 = 0x80;
const ATTR_END: u32 = 0xFFFF_FFFF;

#[cfg(test)]
pub const NS_POSIX: u8 = 0;
#[cfg(test)]
pub const NS_WIN32: u8 = 1;
pub const NS_DOS: u8 = 2;
#[cfg(test)]
pub const NS_WIN32_AND_DOS: u8 = 3;

pub fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
pub fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
pub fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

/// Applies the update-sequence fixups in place. False if the record is not
/// a FILE record or a sector was torn (its check value does not match).
pub fn apply_fixups(buf: &mut [u8]) -> bool {
    if buf.get(0..4) != Some(b"FILE") {
        return false;
    }
    let (Some(usa_off), Some(usa_count)) = (u16_at(buf, 4), u16_at(buf, 6)) else {
        return false;
    };
    let (usa_off, usa_count) = (usa_off as usize, usa_count as usize);
    if usa_count < 2
        || (usa_count - 1) * FIXUP_STRIDE > buf.len()
        || usa_off + usa_count * 2 > buf.len()
    {
        return false;
    }
    let check = [buf[usa_off], buf[usa_off + 1]];
    for i in 1..usa_count {
        let end = i * FIXUP_STRIDE - 2;
        if buf[end..end + 2] != check {
            return false;
        }
        let fix = usa_off + i * 2;
        buf[end] = buf[fix];
        buf[end + 1] = buf[fix + 1];
    }
    true
}

/// One attribute inside a record (offsets are into the record buffer).
#[derive(Debug, Clone, Copy)]
pub struct Attr {
    pub kind: u32,
    pub off: usize,
    pub len: usize,
    pub non_resident: bool,
    pub name_len: u8,
    pub flags: u16,
}

impl Attr {
    /// Resident value as (offset, length) into the record.
    pub fn value(&self, buf: &[u8]) -> Option<(usize, usize)> {
        if self.non_resident {
            return None;
        }
        let len = u32_at(buf, self.off + 0x10)? as usize;
        let off = self.off + u16_at(buf, self.off + 0x14)? as usize;
        (off + len <= self.off + self.len).then_some((off, len))
    }
}

/// Attributes of a fixed-up record, stopping at the end marker or at the
/// first malformed one.
pub fn attributes(buf: &[u8]) -> impl Iterator<Item = Attr> + '_ {
    let end = u32_at(buf, 0x18).map_or(0, |u| (u as usize).min(buf.len()));
    let mut off = u16_at(buf, 0x14).map_or(usize::MAX, usize::from);
    std::iter::from_fn(move || {
        let kind = u32_at(buf, off)?;
        if kind == ATTR_END || off + 0x18 > end {
            return None;
        }
        let len = u32_at(buf, off + 4)? as usize;
        if len < 0x18 || off + len > end {
            return None;
        }
        let a = Attr {
            kind,
            off,
            len,
            non_resident: buf[off + 8] != 0,
            name_len: buf[off + 9],
            flags: u16_at(buf, off + 0x0C)?,
        };
        off += len;
        Some(a)
    })
}

/// A `$FILE_NAME`: the name is UTF-16 at `name_off..name_off + 2 * name_len`.
#[derive(Debug, Clone, Copy)]
pub struct NameRef {
    pub parent: u64,
    pub parent_seq: u16,
    pub namespace: u8,
    pub name_off: usize,
    pub name_len: usize,
}

impl NameRef {
    pub fn decode(&self, buf: &[u8]) -> String {
        let units: Vec<u16> = buf[self.name_off..self.name_off + self.name_len * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        String::from_utf16_lossy(&units)
    }
}

/// What one record says. Reused between records to avoid allocations.
#[derive(Debug, Default)]
pub struct Parsed {
    pub in_use: bool,
    pub is_dir: bool,
    pub seq: u16,
    /// Base record number for an extension record, `None` for a base record.
    pub base: Option<u64>,
    pub names: Vec<NameRef>,
    /// (logical size, allocated size) of the unnamed `$DATA` stream.
    pub data: Option<(u64, u64)>,
    /// Last write time, seconds since the Unix epoch.
    pub modified: Option<u64>,
}

/// Parses one FILE record (applying fixups in place). False if it is not a
/// valid record; a valid record may still be `!in_use`.
pub fn parse_record(buf: &mut [u8], out: &mut Parsed) -> bool {
    out.names.clear();
    out.data = None;
    out.modified = None;
    out.base = None;
    out.in_use = false;
    out.is_dir = false;
    if !apply_fixups(buf) {
        return false;
    }
    let buf = &*buf;
    let flags = u16_at(buf, 0x16).unwrap_or(0);
    out.seq = u16_at(buf, 0x10).unwrap_or(0);
    out.in_use = flags & 0x01 != 0;
    out.is_dir = flags & 0x02 != 0;
    if !out.in_use {
        return true;
    }
    let base = u64_at(buf, 0x20).unwrap_or(0) & REF_MASK;
    out.base = (base != 0).then_some(base);

    for a in attributes(buf) {
        match a.kind {
            ATTR_STANDARD_INFORMATION => {
                if let Some(ft) =
                    a.value(buf)
                        .and_then(|(v, l)| if l >= 0x10 { u64_at(buf, v + 8) } else { None })
                {
                    out.modified = Some(filetime_to_unix(ft));
                }
            }
            ATTR_FILE_NAME => {
                let Some((v, l)) = a.value(buf) else { continue };
                if l < 0x42 {
                    continue;
                }
                let parent = u64_at(buf, v).unwrap_or(0);
                let name_len = buf[v + 0x40] as usize;
                if 0x42 + name_len * 2 > l {
                    continue;
                }
                out.names.push(NameRef {
                    parent: parent & REF_MASK,
                    parent_seq: (parent >> 48) as u16,
                    namespace: buf[v + 0x41],
                    name_off: v + 0x42,
                    name_len,
                });
            }
            ATTR_DATA if a.name_len == 0 => {
                if let Some(d) = data_sizes(buf, &a) {
                    out.data = Some(d);
                }
            }
            _ => {}
        }
    }
    true
}

/// (logical, allocated) size of a `$DATA` attribute; `None` for a
/// non-resident extent that does not start at VCN 0 (only the first extent
/// carries the sizes).
fn data_sizes(buf: &[u8], a: &Attr) -> Option<(u64, u64)> {
    if !a.non_resident {
        // Stored inside the record itself: no clusters of its own.
        let (_, len) = a.value(buf)?;
        return Some((len as u64, 0));
    }
    if u64_at(buf, a.off + 0x10)? != 0 {
        return None;
    }
    let runs_off = u16_at(buf, a.off + 0x20)? as usize;
    let mut allocated = u64_at(buf, a.off + 0x28)?;
    let size = u64_at(buf, a.off + 0x30)?;
    // Compressed or sparse streams have a 0x48-byte header whose last field
    // is the space actually allocated.
    if a.flags & 0x8001 != 0 && runs_off >= 0x48 {
        allocated = u64_at(buf, a.off + 0x40)?;
    }
    Some((size, allocated))
}

/// Non-resident attribute: (starting VCN, data size, runs).
pub fn non_resident_runs(buf: &[u8], a: &Attr) -> Option<(u64, u64, Vec<Run>)> {
    if !a.non_resident {
        return None;
    }
    let start_vcn = u64_at(buf, a.off + 0x10)?;
    let runs_off = u16_at(buf, a.off + 0x20)? as usize;
    let size = u64_at(buf, a.off + 0x30)?;
    let runs = decode_runs(buf.get(a.off + runs_off..a.off + a.len)?)?;
    Some((start_vcn, size, runs))
}

/// A data run: `len` clusters starting at `lcn` (`None` = sparse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub lcn: Option<u64>,
    pub len: u64,
}

/// Decodes an NTFS mapping-pairs array.
pub fn decode_runs(mut b: &[u8]) -> Option<Vec<Run>> {
    let mut runs = Vec::new();
    let mut lcn: i64 = 0;
    while let Some(&head) = b.first() {
        if head == 0 {
            break;
        }
        let (ls, os) = ((head & 0x0F) as usize, (head >> 4) as usize);
        if ls == 0 || ls > 8 || os > 8 || b.len() < 1 + ls + os {
            return None;
        }
        let mut len = 0u64;
        for (i, &x) in b[1..1 + ls].iter().enumerate() {
            len |= u64::from(x) << (8 * i);
        }
        let run_lcn = if os == 0 {
            None
        } else {
            let mut delta = 0i64;
            for (i, &x) in b[1 + ls..1 + ls + os].iter().enumerate() {
                delta |= i64::from(x) << (8 * i);
            }
            if os < 8 && b[ls + os] & 0x80 != 0 {
                delta |= -1i64 << (8 * os); // sign-extend
            }
            lcn = lcn.checked_add(delta)?;
            Some(u64::try_from(lcn).ok()?)
        };
        runs.push(Run { lcn: run_lcn, len });
        b = &b[1 + ls + os..];
    }
    Some(runs)
}

pub fn filetime_to_unix(ft: u64) -> u64 {
    (ft / 10_000_000).saturating_sub(11_644_473_600)
}

// ----------------------------------------------------------------------------
// Collecting records into a tree.

const IN_USE: u8 = 1;
const IS_DIR: u8 = 2;

/// Name priority: a better one replaces a worse one, never an equal one, so
/// the first Win32/POSIX name of the base record wins (one name per file
/// even with several hard links).
const RANK_DOS: u8 = 1;
const RANK_EXT: u8 = 2;
const RANK_BASE: u8 = 3;

/// Compact per-record summary (40 bytes) while the table is being read.
#[derive(Clone, Copy, Default)]
struct Rec {
    size: u64,
    allocated: u64,
    modified: u64,
    parent: u32,
    name_off: u32,
    name_len: u16,
    seq: u16,
    parent_seq: u16,
    flags: u8,
    rank: u8,
}

/// Folds parsed records into per-record summaries, then builds nodes.
pub struct Collector {
    recs: Vec<Rec>,
    names: String,
    pub files: u64,
    pub bytes: u64,
}

impl Collector {
    pub fn new(records: usize) -> Collector {
        Collector {
            recs: vec![Rec::default(); records],
            names: String::new(),
            files: 0,
            bytes: 0,
        }
    }

    /// Adds record number `no`, which `parse_record` just parsed from `buf`.
    pub fn add(&mut self, no: u64, buf: &[u8], p: &Parsed) {
        if !p.in_use {
            return;
        }
        let target = p.base.unwrap_or(no);
        let Some(rec) = usize::try_from(target)
            .ok()
            .filter(|&t| t < self.recs.len())
        else {
            return;
        };
        if p.base.is_none() {
            let r = &mut self.recs[rec];
            r.flags = IN_USE | if p.is_dir { IS_DIR } else { 0 };
            r.seq = p.seq;
            if let Some(m) = p.modified {
                r.modified = m;
            }
        }
        // Only one record of a file holds the first extent of its unnamed
        // stream (the one with the sizes), base or extension.
        if let Some((size, allocated)) = p.data {
            let r = &mut self.recs[rec];
            r.size = size;
            r.allocated = allocated;
            self.files += 1;
            self.bytes += size;
        }
        for n in &p.names {
            let rank = match (n.namespace, p.base) {
                (NS_DOS, _) => RANK_DOS,
                (_, Some(_)) => RANK_EXT,
                (_, None) => RANK_BASE,
            };
            if rank <= self.recs[rec].rank || n.parent > u64::from(u32::MAX) {
                continue;
            }
            let name = n.decode(buf);
            let (Ok(off), Ok(len)) = (u32::try_from(self.names.len()), u16::try_from(name.len()))
            else {
                continue;
            };
            self.names.push_str(&name);
            let r = &mut self.recs[rec];
            r.name_off = off;
            r.name_len = len;
            r.parent = n.parent as u32;
            r.parent_seq = n.parent_seq;
            r.rank = rank;
        }
    }

    /// Builds nodes for the subtree at record `top` (root node named
    /// `root_name`) in breadth-first order, so parents precede children.
    /// Records that do not reach `top` through in-use folders are left out.
    pub fn build(self, top: u64, root_name: &str) -> Option<Vec<Node>> {
        let recs = self.recs;
        let n = recs.len();
        let top = usize::try_from(top)
            .ok()
            .filter(|&t| t < n && recs[t].flags & IS_DIR != 0)?;
        let valid_child = |i: usize| -> Option<usize> {
            let r = &recs[i];
            if r.flags & IN_USE == 0 || r.rank == 0 || i == ROOT_RECORD as usize {
                return None;
            }
            if (i as u64) < FIRST_USER_RECORD {
                return None;
            }
            let p = r.parent as usize;
            let pr = recs.get(p)?;
            let seq_ok = r.parent_seq == 0 || pr.seq == 0 || pr.seq == r.parent_seq;
            (pr.flags & (IN_USE | IS_DIR) == IN_USE | IS_DIR && seq_ok && p != i).then_some(p)
        };

        // Children per record, CSR layout.
        let mut start = vec![0u32; n + 1];
        for i in 0..n {
            if let Some(p) = valid_child(i) {
                start[p + 1] += 1;
            }
        }
        for i in 1..=n {
            start[i] += start[i - 1];
        }
        let mut fill = start.clone();
        let mut list = vec![0u32; start[n] as usize];
        for i in 0..n {
            if let Some(p) = valid_child(i) {
                list[fill[p] as usize] = i as u32;
                fill[p] += 1;
            }
        }
        drop(fill);

        let names = self.names;
        let name_of =
            |r: &Rec| &names[r.name_off as usize..r.name_off as usize + r.name_len as usize];
        let mut nodes = Vec::new();
        let mut order: Vec<u32> = vec![top as u32]; // record of nodes[i]
        nodes.push(leaf(root_name.into(), 0, true, 0, 0, recs[top].modified));
        let mut i = 0;
        while i < order.len() {
            let rec = order[i] as usize;
            for &c in &list[start[rec] as usize..start[rec + 1] as usize] {
                let r = &recs[c as usize];
                let is_dir = r.flags & IS_DIR != 0;
                nodes.push(leaf(
                    name_of(r).into(),
                    i as NodeId,
                    is_dir,
                    r.size,
                    r.allocated,
                    r.modified,
                ));
                order.push(c);
            }
            i += 1;
        }
        Some(nodes)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds synthetic FILE records the way NTFS lays them out.
    pub struct RecordBuilder {
        buf: Vec<u8>,
        attrs: Vec<u8>,
        flags: u16,
        seq: u16,
        base: u64,
    }

    impl RecordBuilder {
        pub fn new() -> Self {
            RecordBuilder {
                buf: vec![0; 1024],
                attrs: Vec::new(),
                flags: 1,
                seq: 1,
                base: 0,
            }
        }
        pub fn dir(mut self) -> Self {
            self.flags |= 2;
            self
        }
        pub fn unused(mut self) -> Self {
            self.flags &= !1;
            self
        }
        pub fn seq(mut self, s: u16) -> Self {
            self.seq = s;
            self
        }
        pub fn extension_of(mut self, base: u64) -> Self {
            self.base = base | (1u64 << 48);
            self
        }
        fn attr(
            mut self,
            kind: u32,
            non_resident: bool,
            name: &str,
            body: &[u8],
            flags: u16,
        ) -> Self {
            let name16: Vec<u8> = name.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            // A non-resident body is the whole header plus runs, from the caller.
            let mut a = if non_resident {
                body.to_vec()
            } else {
                vec![0u8; 0x18]
            };
            let name_off = a.len();
            a.extend_from_slice(&name16);
            while a.len() % 8 != 0 {
                a.push(0);
            }
            let value_off = a.len();
            if !non_resident {
                a.extend_from_slice(body);
                while a.len() % 8 != 0 {
                    a.push(0);
                }
            }
            a[0..4].copy_from_slice(&kind.to_le_bytes());
            let len = a.len() as u32;
            a[4..8].copy_from_slice(&len.to_le_bytes());
            a[8] = non_resident as u8;
            a[9] = name.encode_utf16().count() as u8;
            a[0x0A..0x0C].copy_from_slice(&(name_off as u16).to_le_bytes());
            a[0x0C..0x0E].copy_from_slice(&flags.to_le_bytes());
            if !non_resident {
                a[0x10..0x14].copy_from_slice(&(body.len() as u32).to_le_bytes());
                a[0x14..0x16].copy_from_slice(&(value_off as u16).to_le_bytes());
            }
            self.attrs.extend_from_slice(&a);
            self
        }
        pub fn std_info(self, modified_filetime: u64) -> Self {
            let mut v = vec![0u8; 0x48];
            v[8..16].copy_from_slice(&modified_filetime.to_le_bytes());
            self.attr(ATTR_STANDARD_INFORMATION, false, "", &v, 0)
        }
        pub fn file_name(self, parent: u64, parent_seq: u16, ns: u8, name: &str) -> Self {
            let n16: Vec<u8> = name.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            let mut v = vec![0u8; 0x42];
            v[0..8].copy_from_slice(&(parent | (u64::from(parent_seq) << 48)).to_le_bytes());
            v[0x40] = name.encode_utf16().count() as u8;
            v[0x41] = ns;
            v.extend_from_slice(&n16);
            self.attr(ATTR_FILE_NAME, false, "", &v, 0)
        }
        pub fn resident_data(self, stream: &str, len: usize) -> Self {
            self.attr(ATTR_DATA, false, stream, &vec![0xAB; len], 0)
        }
        /// Non-resident `$DATA`, optionally compressed (0x48 header with
        /// the real allocation at 0x40).
        pub fn nonresident_data(
            self,
            start_vcn: u64,
            size: u64,
            allocated: u64,
            compressed: Option<u64>,
            runs: &[u8],
        ) -> Self {
            let hdr = if compressed.is_some() { 0x48 } else { 0x40 };
            let mut v = vec![0u8; hdr];
            v[0x10..0x18].copy_from_slice(&start_vcn.to_le_bytes());
            v[0x20..0x22].copy_from_slice(&(hdr as u16).to_le_bytes());
            v[0x28..0x30].copy_from_slice(&allocated.to_le_bytes());
            v[0x30..0x38].copy_from_slice(&size.to_le_bytes());
            v[0x38..0x40].copy_from_slice(&size.to_le_bytes());
            if let Some(c) = compressed {
                v[0x40..0x48].copy_from_slice(&c.to_le_bytes());
            }
            v.extend_from_slice(runs);
            v.push(0);
            let flags = if compressed.is_some() { 0x0001 } else { 0 };
            self.attr(ATTR_DATA, true, "", &v, flags)
        }

        /// The finished 1024-byte record with fixups applied (as on disk).
        pub fn build(mut self) -> Vec<u8> {
            let b = &mut self.buf;
            b[0..4].copy_from_slice(b"FILE");
            b[4..6].copy_from_slice(&0x30u16.to_le_bytes()); // USA offset
            b[6..8].copy_from_slice(&3u16.to_le_bytes()); // 1 + 2 sectors
            b[0x10..0x12].copy_from_slice(&self.seq.to_le_bytes());
            b[0x12..0x14].copy_from_slice(&1u16.to_le_bytes());
            b[0x14..0x16].copy_from_slice(&0x38u16.to_le_bytes());
            b[0x16..0x18].copy_from_slice(&self.flags.to_le_bytes());
            let mut body = self.attrs.clone();
            body.extend_from_slice(&ATTR_END.to_le_bytes());
            body.extend_from_slice(&[0; 4]);
            let used = 0x38 + body.len();
            assert!(used <= 1024 - 2, "record too full for the test builder");
            b[0x38..used].copy_from_slice(&body);
            b[0x18..0x1C].copy_from_slice(&(used as u32).to_le_bytes());
            b[0x1C..0x20].copy_from_slice(&1024u32.to_le_bytes());
            b[0x20..0x28].copy_from_slice(&self.base.to_le_bytes());
            // Update sequence: move each sector's last two bytes into the array.
            let usn = [0x5Au8, 0x01];
            b[0x30..0x32].copy_from_slice(&usn);
            for i in 1..3 {
                let end = i * 512 - 2;
                let (x, y) = (b[end], b[end + 1]);
                b[0x30 + i * 2] = x;
                b[0x30 + i * 2 + 1] = y;
                b[end..end + 2].copy_from_slice(&usn);
            }
            self.buf
        }
    }

    fn parse(mut rec: Vec<u8>) -> (Vec<u8>, Parsed, bool) {
        let mut p = Parsed::default();
        let ok = parse_record(&mut rec, &mut p);
        (rec, p, ok)
    }

    #[test]
    fn fixups_restore_sector_tails_and_catch_torn_writes() {
        // A name long enough to cross the first sector boundary.
        let long = "x".repeat(250);
        let rec = RecordBuilder::new()
            .file_name(5, 5, NS_WIN32, &long)
            .build();
        assert_eq!(
            &rec[510..512],
            &[0x5A, 0x01],
            "builder stored the check value"
        );
        let (buf, p, ok) = parse(rec.clone());
        assert!(ok && p.in_use);
        assert_eq!(p.names[0].decode(&buf), long);

        let mut torn = rec;
        torn[1022] ^= 0xFF;
        let (_, _, ok) = parse(torn);
        assert!(!ok, "a mismatched check value must reject the record");

        let mut bad = RecordBuilder::new().build();
        bad[0..4].copy_from_slice(b"BAAD");
        assert!(!parse(bad).2);
    }

    #[test]
    fn resident_and_nonresident_data() {
        let (_, p, ok) = parse(
            RecordBuilder::new()
                .std_info(133_000_000_000_000_000)
                .resident_data("", 300)
                .build(),
        );
        assert!(ok);
        assert_eq!(p.data, Some((300, 0)));
        assert_eq!(p.modified, Some(filetime_to_unix(133_000_000_000_000_000)));

        let (_, p, _) = parse(
            RecordBuilder::new()
                .nonresident_data(0, 10_000, 12_288, None, &[0x21, 0x03, 0x10, 0x00])
                .build(),
        );
        assert_eq!(p.data, Some((10_000, 12_288)));

        // Compressed: the real allocation comes from the 0x40 field.
        let (_, p, _) = parse(
            RecordBuilder::new()
                .nonresident_data(0, 100_000, 102_400, Some(40_960), &[0x11, 0x0A, 0x20])
                .build(),
        );
        assert_eq!(p.data, Some((100_000, 40_960)));

        // A later extent (VCN > 0) does not carry sizes.
        let (_, p, _) = parse(
            RecordBuilder::new()
                .nonresident_data(7, 1, 1, None, &[0x11, 0x01, 0x05])
                .build(),
        );
        assert_eq!(p.data, None);

        // Named streams (alternate data streams) are ignored.
        let (_, p, _) = parse(
            RecordBuilder::new()
                .resident_data("Zone.Identifier", 26)
                .build(),
        );
        assert_eq!(p.data, None);
    }

    #[test]
    fn data_runs_decode_with_signed_offsets_and_sparse() {
        // 3 clusters at 0x10, then 2 clusters 4 back (0x0C), then 5 sparse.
        let runs =
            decode_runs(&[0x21, 0x03, 0x10, 0x00, 0x11, 0x02, 0xFC, 0x01, 0x05, 0x00]).unwrap();
        assert_eq!(
            runs,
            vec![
                Run {
                    lcn: Some(0x10),
                    len: 3
                },
                Run {
                    lcn: Some(0x0C),
                    len: 2
                },
                Run { lcn: None, len: 5 },
            ]
        );
        assert!(decode_runs(&[0x33, 0x01]).is_none(), "truncated run");
    }

    #[test]
    fn unused_record_is_flagged_not_in_use() {
        let (_, p, ok) = parse(
            RecordBuilder::new()
                .unused()
                .file_name(5, 5, NS_WIN32, "gone.txt")
                .build(),
        );
        assert!(ok);
        assert!(!p.in_use);
        assert!(p.names.is_empty());
    }

    fn feed(c: &mut Collector, no: u64, rec: Vec<u8>) {
        let mut rec = rec;
        let mut p = Parsed::default();
        assert!(parse_record(&mut rec, &mut p));
        c.add(no, &rec, &p);
    }

    #[test]
    fn collector_builds_tree_with_names_links_and_extensions() {
        let mut c = Collector::new(64);
        // Root folder (record 5, its own parent) and a metadata file.
        feed(
            &mut c,
            5,
            RecordBuilder::new()
                .dir()
                .seq(5)
                .file_name(5, 5, NS_WIN32_AND_DOS, ".")
                .build(),
        );
        feed(
            &mut c,
            2,
            RecordBuilder::new()
                .seq(2)
                .file_name(5, 5, NS_WIN32_AND_DOS, "$LogFile")
                .resident_data("", 64)
                .build(),
        );
        // A folder with a long name: DOS name first, Win32 name second.
        feed(
            &mut c,
            30,
            RecordBuilder::new()
                .dir()
                .seq(3)
                .std_info(0)
                .file_name(5, 5, NS_DOS, "PROGRA~1")
                .file_name(5, 5, NS_WIN32, "Program Files")
                .build(),
        );
        // Extension record arrives before its base: holds the $DATA.
        feed(
            &mut c,
            40,
            RecordBuilder::new()
                .extension_of(31)
                .nonresident_data(0, 5000, 8192, None, &[0x11, 0x02, 0x30])
                .build(),
        );
        feed(
            &mut c,
            31,
            RecordBuilder::new()
                .seq(9)
                .file_name(30, 3, NS_POSIX, "big.dat")
                .build(),
        );
        // A file with two hard links: counted once, under the first name.
        feed(
            &mut c,
            32,
            RecordBuilder::new()
                .seq(1)
                .file_name(30, 3, NS_WIN32_AND_DOS, "first.txt")
                .file_name(5, 5, NS_WIN32_AND_DOS, "second.txt")
                .resident_data("", 10)
                .build(),
        );
        // Not in use: skipped.
        feed(
            &mut c,
            33,
            RecordBuilder::new()
                .unused()
                .file_name(5, 5, NS_WIN32, "deleted")
                .resident_data("", 9)
                .build(),
        );
        // Orphan whose parent record was reused (sequence mismatch).
        feed(
            &mut c,
            34,
            RecordBuilder::new()
                .file_name(30, 2, NS_WIN32, "orphan")
                .resident_data("", 7)
                .build(),
        );

        let nodes = c.build(ROOT_RECORD, r"C:\").unwrap();
        let tree = crate::finish(
            nodes,
            crate::Method::FileTable,
            Vec::new(),
            &crate::Progress::default(),
        );
        let root = &tree.nodes[0];
        assert_eq!(&*root.name, r"C:\");
        assert_eq!(root.size, 5010);
        assert_eq!(root.allocated, 8192);
        assert_eq!(root.files, 2);
        assert_eq!(root.dirs, 1);
        let kids: Vec<&str> = tree
            .children(0)
            .iter()
            .map(|&i| &*tree.nodes[i as usize].name)
            .collect();
        assert_eq!(kids, ["Program Files"]);
        let pf = tree
            .find(std::path::Path::new(r"c:\program files"))
            .unwrap();
        let kids: Vec<&str> = tree
            .children(pf)
            .iter()
            .map(|&i| &*tree.nodes[i as usize].name)
            .collect();
        assert_eq!(kids, ["big.dat", "first.txt"]);
        assert_eq!(
            tree.path(tree.children(pf)[0]),
            std::path::PathBuf::from(r"C:\Program Files\big.dat")
        );

        // Re-rooted at the subfolder.
        let mut c2 = Collector::new(64);
        feed(
            &mut c2,
            5,
            RecordBuilder::new()
                .dir()
                .seq(5)
                .file_name(5, 5, NS_WIN32_AND_DOS, ".")
                .build(),
        );
        feed(
            &mut c2,
            30,
            RecordBuilder::new()
                .dir()
                .seq(3)
                .file_name(5, 5, NS_WIN32, "Program Files")
                .build(),
        );
        feed(
            &mut c2,
            31,
            RecordBuilder::new()
                .file_name(30, 3, NS_WIN32, "a.bin")
                .resident_data("", 100)
                .build(),
        );
        let nodes = c2.build(30, r"C:\Program Files").unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(&*nodes[0].name, r"C:\Program Files");
        assert_eq!(&*nodes[1].name, "a.bin");
        assert!(
            Collector::new(8).build(5, "x").is_none(),
            "missing root record"
        );
    }
}
