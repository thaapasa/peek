//! LMDB on-disk format: constants plus zero-copy views over meta, page and
//! node bytes, for both formats a 64-bit little-endian build writes —
//! every x86_64 / arm64 file in practice:
//!
//! - **0.9.x**, data version 1: 16-byte page header, node offsets from the
//!   page start, node data right after the key.
//! - **1.0.x**, data version 3: 24-byte page header (adds a page txnid),
//!   `lower` / `upper` / node offsets relative to the header end
//!   (`PAGEBASE`), node data after the key padded to even length.
//!
//! The [`Variant`] carries those three differences; everything else is
//! shared. 32-bit and big-endian builds, other data versions and encrypted
//! 1.0 environments are recognised by [`classify_layout`] / [`Meta::parse`]
//! so Info can name them, but not walked.
//!
//! Layout (offsets in bytes, `H` = header size 16 / 24, all little-endian):
//!
//! ```text
//!  page    : pgno u64 │ [txnid u64, 1.0] │ pad u16 │ flags u16 │
//!            lower u16 │ upper u16 │ node offsets u16[]…
//!            overflow pages reuse lower+upper as `pages u32`; data at H
//!  meta    : page 0 and page psize; body at H:
//!            magic u32 │ version u32 │ addr u64 │ mapsize u64 │
//!            dbs[FREE, MAIN] │ last_pg u64 │ txnid u64
//!  db      : pad u32 │ flags u16 │ depth u16 │ branch_pages u64 │
//!            leaf_pages u64 │ overflow_pages u64 │ entries u64 │ root u64
//!            dbs[FREE].pad = page size, dbs[FREE].flags = env flags
//!  node    : lo u16 │ hi u16 │ flags u16 │ ksize u16 │ key │ data
//!            branch child pgno = lo | hi<<16 | flags<<32
//!            leaf data size    = lo | hi<<16
//!  LEAF2   : fixed-size keys packed from H, no nodes (DUPFIXED dups)
//! ```

use std::cmp::Ordering;

use anyhow::{Result, anyhow, ensure};
use bytes::Bytes;

pub const MAGIC: u32 = 0xBEEF_C0DE;
const NODE_HEADER: usize = 8;
pub const DB_RECORD_LEN: usize = 48;

const P_BRANCH: u16 = 0x01;
const P_LEAF: u16 = 0x02;
const P_OVERFLOW: u16 = 0x04;
const P_META: u16 = 0x08;
const P_LEAF2: u16 = 0x20;

/// Leaf node: data is a `u64` overflow page number.
pub const F_BIGDATA: u16 = 0x01;
/// Leaf node: data is a DB record (named DB, or a dupsort sub-tree).
pub const F_SUBDATA: u16 = 0x02;
/// Leaf node: data holds duplicate values (sub-page or sub-tree).
pub const F_DUPDATA: u16 = 0x04;

pub const MDB_REVERSEKEY: u16 = 0x02;
pub const MDB_DUPSORT: u16 = 0x04;
pub const MDB_INTEGERKEY: u16 = 0x08;
pub const MDB_DUPFIXED: u16 = 0x10;
pub const MDB_INTEGERDUP: u16 = 0x20;
pub const MDB_REVERSEDUP: u16 = 0x40;

const DB_FLAG_NAMES: &[(u16, &str)] = &[
    (MDB_REVERSEKEY, "reversekey"),
    (MDB_DUPSORT, "dupsort"),
    (MDB_INTEGERKEY, "integerkey"),
    (MDB_DUPFIXED, "dupfixed"),
    (MDB_INTEGERDUP, "integerdup"),
    (MDB_REVERSEDUP, "reversedup"),
];

/// Env flag (in `dbs[FREE].flags`) marking a 1.0 encrypted environment.
const MDB_ENCRYPT: u16 = 0x2000;

// Meta fields, relative to the page header end.
const META_MAGIC: usize = 0;
const META_VERSION: usize = 4;
const META_MAPSIZE: usize = 16;
const META_FREE_DB: usize = 24;
const META_MAIN_DB: usize = 72;
const META_LAST_PG: usize = 120;
const META_TXNID: usize = 128;
const META_BODY: usize = 136;
/// Bytes of page 0 / page 1 any variant's meta parse needs.
pub const META_LEN: usize = 24 + META_BODY;

/// The per-format layout differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variant {
    /// Data version in the meta page.
    pub version: u32,
    /// Page header size; also where a page's node offsets / LEAF2 keys /
    /// overflow data start.
    pub header: usize,
    /// What `lower`, `upper` and node offsets are relative to.
    pub(crate) base: usize,
    /// Node data starts after the key padded to even length.
    even_keys: bool,
}

/// LMDB 0.9.x.
pub const V09: Variant = Variant {
    version: 1,
    header: 16,
    base: 0,
    even_keys: false,
};

/// LMDB 1.0.x.
pub const V10: Variant = Variant {
    version: 3,
    header: 24,
    base: 24,
    even_keys: true,
};

impl Variant {
    pub fn label(self) -> &'static str {
        if self == V10 { "1.0" } else { "0.9" }
    }
}

fn le16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn le32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

pub fn le64(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

/// What the head of a file claims to be. Only [`Layout::Native`] is walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// 64-bit little-endian 0.9 or 1.0.
    Native(Variant),
    /// 32-bit build: 12-byte page header and 4-byte size fields.
    Bits32,
    /// Written on a big-endian host.
    BigEndian,
    /// Native layout, unknown data version.
    Version(u32),
    /// No LMDB magic.
    Unknown,
}

impl Layout {
    /// Why a non-native layout can't be read, for Info.
    pub fn unsupported_reason(self) -> String {
        match self {
            Layout::Native(_) => "supported".to_string(),
            Layout::Bits32 => "32-bit LMDB layout is not supported".to_string(),
            Layout::BigEndian => "big-endian LMDB layout is not supported".to_string(),
            Layout::Version(v) => format!("LMDB data version {v} is not supported"),
            Layout::Unknown => "no LMDB meta page found".to_string(),
        }
    }
}

/// Classify a file head by where the meta magic sits and its byte order.
pub fn classify_layout(head: &[u8]) -> Layout {
    for v in [V09, V10] {
        if le32(head, v.header + META_MAGIC) == Some(MAGIC) {
            return match le32(head, v.header + META_VERSION) {
                Some(version) if version == v.version => Layout::Native(v),
                Some(version) => Layout::Version(version),
                None => Layout::Unknown,
            };
        }
    }
    if [12, 16, 24].iter().any(|&o| be32(head, o) == Some(MAGIC)) {
        return Layout::BigEndian;
    }
    if le32(head, 12) == Some(MAGIC) {
        return Layout::Bits32;
    }
    Layout::Unknown
}

/// Per-database B+tree record: stats plus the root page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbRecord {
    /// FREE DB: page size. DUPFIXED sub-DB: fixed value size.
    pub pad: u32,
    pub flags: u16,
    pub depth: u16,
    pub branch_pages: u64,
    pub leaf_pages: u64,
    pub overflow_pages: u64,
    pub entries: u64,
    /// `None` for an empty tree (`P_INVALID` on disk).
    pub root: Option<u64>,
}

impl DbRecord {
    pub fn parse(b: &[u8]) -> Option<Self> {
        let root = le64(b, 40)?;
        Some(Self {
            pad: le32(b, 0)?,
            flags: le16(b, 4)?,
            depth: le16(b, 6)?,
            branch_pages: le64(b, 8)?,
            leaf_pages: le64(b, 16)?,
            overflow_pages: le64(b, 24)?,
            entries: le64(b, 32)?,
            root: (root != u64::MAX).then_some(root),
        })
    }

    pub fn is_dupsort(&self) -> bool {
        self.flags & MDB_DUPSORT != 0
    }

    pub fn total_pages(&self) -> u64 {
        self.branch_pages
            .saturating_add(self.leaf_pages)
            .saturating_add(self.overflow_pages)
    }

    /// Names of the set user-visible flags, in bit order.
    pub fn flag_names(&self) -> Vec<&'static str> {
        DB_FLAG_NAMES
            .iter()
            .filter(|(bit, _)| self.flags & bit != 0)
            .map(|&(_, name)| name)
            .collect()
    }
}

/// One meta page: environment-wide state at a committed transaction.
#[derive(Debug, Clone, Copy)]
pub struct Meta {
    pub mapsize: u64,
    pub free: DbRecord,
    pub main: DbRecord,
    pub last_pg: u64,
    pub txnid: u64,
}

impl Meta {
    pub fn parse(page: &[u8], v: Variant) -> Result<Self> {
        let hdr = PageHeader::parse(page, v).ok_or_else(|| anyhow!("meta page truncated"))?;
        let h = v.header;
        ensure!(hdr.flags & P_META != 0, "page is not a meta page");
        ensure!(le32(page, h + META_MAGIC) == Some(MAGIC), "bad meta magic");
        ensure!(
            le32(page, h + META_VERSION) == Some(v.version),
            "unsupported data version"
        );
        let field = |o| le64(page, h + o).ok_or_else(|| anyhow!("meta page truncated"));
        let db = |o: usize| {
            page.get(h + o..h + o + DB_RECORD_LEN)
                .and_then(DbRecord::parse)
                .ok_or_else(|| anyhow!("meta page truncated"))
        };
        let meta = Self {
            mapsize: field(META_MAPSIZE)?,
            free: db(META_FREE_DB)?,
            main: db(META_MAIN_DB)?,
            last_pg: field(META_LAST_PG)?,
            txnid: field(META_TXNID)?,
        };
        ensure!(
            valid_page_size(meta.page_size()),
            "implausible page size {}",
            meta.page_size()
        );
        ensure!(
            meta.free.flags & MDB_ENCRYPT == 0,
            "encrypted LMDB environment is not supported"
        );
        Ok(meta)
    }

    pub fn page_size(&self) -> u64 {
        u64::from(self.free.pad)
    }
}

/// LMDB uses the OS page size: a power of two, 4 KiB on x86, 16 KiB on
/// Apple Silicon, up to 64 KiB on some arm64 / POWER kernels.
pub fn valid_page_size(psize: u64) -> bool {
    psize.is_power_of_two() && (512..=65536).contains(&psize)
}

/// A page header's fields, read per [`Variant`].
#[derive(Debug, Clone, Copy)]
pub struct PageHeader {
    pub pad: u16,
    pub flags: u16,
    /// End of the node-offset array, absolute from the page start.
    lower_abs: usize,
    v: Variant,
}

impl PageHeader {
    pub fn parse(b: &[u8], v: Variant) -> Option<Self> {
        let at = v.header - 8;
        Some(Self {
            pad: le16(b, at)?,
            flags: le16(b, at + 2)?,
            lower_abs: v.base + usize::from(le16(b, at + 4)?),
            v,
        })
    }

    pub fn is_branch(&self) -> bool {
        self.flags & P_BRANCH != 0
    }

    pub fn is_leaf(&self) -> bool {
        self.flags & P_LEAF != 0
    }

    pub fn is_leaf2(&self) -> bool {
        self.flags & P_LEAF2 != 0
    }

    pub fn is_overflow(&self) -> bool {
        self.flags & P_OVERFLOW != 0
    }

    /// Entry count of a branch / leaf page. Trusts `lower` — callers
    /// holding the full page go through [`Page::new`], which checks it.
    pub fn num_keys(&self) -> usize {
        self.lower_abs.saturating_sub(self.v.header) / 2
    }
}

/// A full branch / leaf page, or an inline dupsort sub-page (same header,
/// offsets relative to the sub-page start).
#[derive(Clone)]
pub struct Page {
    pub hdr: PageHeader,
    buf: Bytes,
}

impl Page {
    pub fn new(buf: Bytes, v: Variant) -> Result<Self> {
        let hdr = PageHeader::parse(&buf, v).ok_or_else(|| anyhow!("page truncated"))?;
        ensure!(
            hdr.lower_abs >= v.header && hdr.lower_abs <= buf.len(),
            "page lower bound {} out of range",
            hdr.lower_abs
        );
        Ok(Self { hdr, buf })
    }

    pub fn num_keys(&self) -> usize {
        self.hdr.num_keys()
    }

    /// Node `i` of a branch / leaf page.
    pub fn node(&self, i: usize) -> Result<Node> {
        ensure!(i < self.num_keys(), "node {i} out of range");
        let v = self.hdr.v;
        let ptr =
            le16(&self.buf, v.header + 2 * i).ok_or_else(|| anyhow!("node offset truncated"))?;
        let off = v.base + usize::from(ptr);
        ensure!(
            off >= self.hdr.lower_abs && off + NODE_HEADER <= self.buf.len(),
            "node {i} offset {off} out of range"
        );
        let b = &self.buf;
        let lo_hi =
            u32::from(le16(b, off).unwrap_or(0)) | (u32::from(le16(b, off + 2).unwrap_or(0)) << 16);
        let flags = le16(b, off + 4).unwrap_or(0);
        let ksize = usize::from(le16(b, off + 6).unwrap_or(0));
        let key_end = off + NODE_HEADER + ksize;
        let data_start = if v.even_keys {
            key_end + (ksize & 1)
        } else {
            key_end
        };
        ensure!(data_start <= b.len(), "node {i} key overruns page");
        Ok(Node {
            flags,
            lo_hi,
            key: b.slice(off + NODE_HEADER..key_end),
            rest: b.slice(data_start..),
        })
    }

    /// Key `i` of a LEAF2 page (fixed `ksize`, no node headers).
    pub fn leaf2_key(&self, i: usize, ksize: usize) -> Result<Bytes> {
        ensure!(ksize > 0, "LEAF2 page with zero key size");
        ensure!(i < self.num_keys(), "key {i} out of range");
        let start = self.hdr.v.header + i * ksize;
        ensure!(
            start + ksize <= self.buf.len(),
            "LEAF2 key {i} overruns page"
        );
        Ok(self.buf.slice(start..start + ksize))
    }
}

/// One branch / leaf node.
pub struct Node {
    pub flags: u16,
    lo_hi: u32,
    pub key: Bytes,
    /// Page bytes after the key — the node's data sits at the front.
    rest: Bytes,
}

impl Node {
    /// Child page of a branch node.
    pub fn child_pgno(&self) -> u64 {
        u64::from(self.lo_hi) | (u64::from(self.flags) << 32)
    }

    /// Logical value size of a leaf node (the overflow length for
    /// `F_BIGDATA`).
    pub fn data_size(&self) -> u64 {
        u64::from(self.lo_hi)
    }

    /// The node's inline data: the value itself, an 8-byte overflow pgno
    /// (`F_BIGDATA`), or a 48-byte DB record (`F_SUBDATA`).
    pub fn data(&self) -> Result<Bytes> {
        let len = if self.flags & F_BIGDATA != 0 {
            8
        } else {
            self.lo_hi as usize
        };
        ensure!(len <= self.rest.len(), "node data overruns page");
        Ok(self.rest.slice(..len))
    }
}

/// Key order for a DB's flags, matching LMDB's built-in comparators.
/// INTEGERKEY compares native unsigned ints; REVERSEKEY compares bytes
/// back to front. Custom app comparators aren't stored in the file.
pub fn cmp_keys(flags: u16, a: &[u8], b: &[u8]) -> Ordering {
    if flags & MDB_INTEGERKEY != 0 && a.len() == b.len() && matches!(a.len(), 4 | 8) {
        return int_le(a).cmp(&int_le(b));
    }
    if flags & MDB_REVERSEKEY != 0 {
        return a.iter().rev().cmp(b.iter().rev());
    }
    a.cmp(b)
}

/// A 4- or 8-byte little-endian unsigned int, widened.
pub fn int_le(b: &[u8]) -> u64 {
    match b.len() {
        4 => u64::from(u32::from_le_bytes(b.try_into().unwrap_or_default())),
        8 => u64::from_le_bytes(b.try_into().unwrap_or_default()),
        _ => 0,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn layout_classification() {
        let mut head = vec![0u8; 64];
        head[16..20].copy_from_slice(&MAGIC.to_le_bytes());
        head[20..24].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(classify_layout(&head), Layout::Native(V09));
        head[20..24].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(classify_layout(&head), Layout::Version(2));

        let mut head = vec![0u8; 64];
        head[24..28].copy_from_slice(&MAGIC.to_le_bytes());
        head[28..32].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(classify_layout(&head), Layout::Native(V10));

        let mut head = vec![0u8; 64];
        head[16..20].copy_from_slice(&MAGIC.to_be_bytes());
        assert_eq!(classify_layout(&head), Layout::BigEndian);

        let mut head = vec![0u8; 64];
        head[12..16].copy_from_slice(&MAGIC.to_le_bytes());
        assert_eq!(classify_layout(&head), Layout::Bits32);

        assert_eq!(classify_layout(b"short"), Layout::Unknown);
    }

    #[test]
    fn comparators_follow_db_flags() {
        let one = 1u64.to_le_bytes();
        let big = 256u64.to_le_bytes();
        // Bytewise, 1 (01 00…) sorts after 256 (00 01…); as ints it's before.
        assert_eq!(cmp_keys(0, &one, &big), Ordering::Greater);
        assert_eq!(cmp_keys(MDB_INTEGERKEY, &one, &big), Ordering::Less);
        assert_eq!(cmp_keys(MDB_REVERSEKEY, b"ab", b"ba"), Ordering::Greater);
    }

    #[test]
    fn node_rejects_offsets_outside_the_page() {
        let mut page = vec![0u8; 64];
        page[12..14].copy_from_slice(&18u16.to_le_bytes()); // lower: 1 key
        page[14..16].copy_from_slice(&60u16.to_le_bytes());
        page[16..18].copy_from_slice(&62u16.to_le_bytes()); // node header overruns
        let p = Page::new(Bytes::from(page), V09).unwrap();
        assert!(p.node(0).is_err());
        assert!(p.node(1).is_err(), "past num_keys");
    }

    /// One leaf node with an odd-length key, laid out per `v`.
    fn one_node_page(v: Variant) -> Page {
        let mut page = vec![0u8; 96];
        let h = v.header;
        let node_abs = 64;
        page[h - 4..h - 2].copy_from_slice(&((h + 2 - v.base) as u16).to_le_bytes()); // lower
        page[h..h + 2].copy_from_slice(&((node_abs - v.base) as u16).to_le_bytes());
        page[node_abs..node_abs + 2].copy_from_slice(&2u16.to_le_bytes()); // dsize 2
        page[node_abs + 6..node_abs + 8].copy_from_slice(&3u16.to_le_bytes()); // ksize 3
        page[node_abs + 8..node_abs + 11].copy_from_slice(b"key");
        let data = node_abs + 11 + usize::from(v.even_keys);
        page[data..data + 2].copy_from_slice(b"ok");
        Page::new(Bytes::from(page), v).unwrap()
    }

    /// A LEAF2 page of `n` packed `ksize`-byte keys (key `i` = `[i; ksize]`),
    /// laid out per `v`.
    pub(crate) fn leaf2_page_bytes(v: Variant, ksize: usize, n: usize) -> Vec<u8> {
        let h = v.header;
        let len = h + n * ksize;
        let mut page = vec![0u8; len];
        page[h - 8..h - 6].copy_from_slice(&(ksize as u16).to_le_bytes()); // pad = key size
        page[h - 6..h - 4].copy_from_slice(&(P_LEAF | P_LEAF2).to_le_bytes());
        page[h - 4..h - 2].copy_from_slice(&((h + 2 * n - v.base) as u16).to_le_bytes()); // lower
        page[h - 2..h].copy_from_slice(&((len - v.base) as u16).to_le_bytes()); // upper
        for i in 0..n {
            page[h + i * ksize..h + (i + 1) * ksize].fill(i as u8);
        }
        page
    }

    #[test]
    fn leaf2_keys_per_variant() {
        for v in [V09, V10] {
            let p = Page::new(Bytes::from(leaf2_page_bytes(v, 4, 3)), v).unwrap();
            assert!(p.hdr.is_leaf2(), "{v:?}");
            assert_eq!(p.num_keys(), 3, "{v:?}");
            assert_eq!(usize::from(p.hdr.pad), 4, "{v:?}");
            for i in 0..3 {
                assert_eq!(&p.leaf2_key(i, 4).unwrap()[..], &[i as u8; 4], "{v:?}");
            }
            assert!(p.leaf2_key(3, 4).is_err(), "past num_keys");
            assert!(p.leaf2_key(0, 0).is_err(), "zero key size");
            assert!(p.leaf2_key(2, 5).is_err(), "key overruns page");
        }
    }

    #[test]
    fn node_layout_per_variant() {
        for v in [V09, V10] {
            let p = one_node_page(v);
            assert_eq!(p.num_keys(), 1, "{v:?}");
            let n = p.node(0).unwrap();
            assert_eq!(&n.key[..], b"key");
            assert_eq!(&n.data().unwrap()[..], b"ok", "{v:?}");
        }
    }
}
