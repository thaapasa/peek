//! `.DS_Store` parser — the "Bud1" Buddy-allocator container.
//!
//! The file is a generic block store (Apple's `BTree`-on-`BuddyAllocator`,
//! shared with alias / bookmark files) holding one B-tree named `DSDB`.
//! Each B-tree record is a `(filename, structure-id, typed value)` triple:
//! the Finder view settings for one entry in the folder. We walk the tree
//! read-only and collect every record; nothing is decoded lazily because
//! the file is tiny (one block per ~few-KB folder).
//!
//! Layout (all integers big-endian):
//!
//! ```text
//!  header   : 0x00000001 │ "Bud1" │ rootOffset │ rootSize │ rootOffset(copy)
//!  block N  : addressed by the allocator's offset table; file offset =
//!             (entry & ~0x1f) + 4, byte length = 1 << (entry & 0x1f)
//!  root blk : offsetCount │ _ │ offsetTable[ceil(count/256)*256] │
//!             dirCount │ {nameLen, name, blockId}* │ freeLists…
//!  DSDB blk : rootNode │ levels │ records │ nodes │ pageSize
//!  node     : next │ count │ (next>0 ? {childId, record}*count + child(next)
//!                                     : record*count)
//!  record   : nameLen(u32, UTF-16 units) │ nameUTF16BE │ id(4) │ type(4) │ value
//! ```
//!
//! The `+4` block-address adjustment is because the leading `0x00000001`
//! word sits outside the allocator's address space (allocator address 0
//! maps to file offset 4).

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow, bail};

/// Cap on B-tree recursion depth — a well-formed store nests only a
/// handful of levels; the bound stops a malformed / cyclic file from
/// overflowing the stack.
const MAX_DEPTH: usize = 32;

/// One parsed `.DS_Store` — the flat list of every B-tree record.
pub struct DsStore {
    pub records: Vec<DsRecord>,
    /// Set when the walk hit a malformed node or an unknown record
    /// encoding and stopped early; the records gathered before that
    /// point are still valid.
    pub truncated: bool,
}

/// One stored property: a Finder setting for the named entry.
pub struct DsRecord {
    /// Filename the setting applies to (a child of the folder, or `.`
    /// for the folder itself).
    pub name: String,
    /// Four-character structure id (`Iloc`, `bwsp`, `vstl`, …).
    pub code: String,
    pub value: DsValue,
}

/// A record's typed value. The variant follows the on-disk 4-byte type
/// tag; `code`-specific decoding (icon coordinates, window frame, view
/// style, background) happens in [`format_value`], not here.
pub enum DsValue {
    /// `long` / `shor` — a 32-bit integer.
    Int(u32),
    /// `bool` — a single byte.
    Bool(bool),
    /// `type` — a four-character code.
    Type(String),
    /// `comp` — a 64-bit integer (sizes, ids).
    Long(u64),
    /// `dutc` — a UTC timestamp (raw, format-dependent epoch).
    Date(u64),
    /// `ustr` — a UTF-16 string.
    Str(String),
    /// `blob` — opaque bytes (icon location, window frame, embedded
    /// binary plists).
    Blob(Vec<u8>),
}

/// Parse `data` (the whole file) into its record list.
pub fn parse(data: &[u8]) -> Result<DsStore> {
    if data.len() < 36 {
        bail!("file too small for a .DS_Store header");
    }
    if &data[0..4] != b"\x00\x00\x00\x01" || &data[4..8] != b"Bud1" {
        bail!("not a Bud1 .DS_Store container");
    }
    let root_off = be_u32(&data[8..12]) as usize;
    let root_size = be_u32(&data[12..16]) as usize;
    // data[16..20] is a copy of root_off; not verified.

    let alloc = Allocator::parse(data, root_off, root_size)?;
    let master = *alloc
        .dir
        .get("DSDB")
        .ok_or_else(|| anyhow!("no DSDB tree in .DS_Store"))?;
    let header = alloc.block(master)?;
    if header.len() < 4 {
        bail!("truncated DSDB header");
    }
    let root_node = be_u32(&header[0..4]);

    let mut store = DsStore {
        records: Vec::new(),
        truncated: false,
    };
    walk(&alloc, root_node, 0, &mut HashSet::new(), &mut store);
    Ok(store)
}

/// The Buddy allocator: the offset table (block id → packed address) plus
/// the directory of named trees.
struct Allocator<'a> {
    data: &'a [u8],
    offsets: Vec<u32>,
    dir: HashMap<String, u32>,
}

impl<'a> Allocator<'a> {
    fn parse(data: &'a [u8], root_off: usize, root_size: usize) -> Result<Self> {
        let start = root_off + 4;
        let blk = data
            .get(start..start + root_size)
            .ok_or_else(|| anyhow!("bookkeeping block out of range"))?;
        let mut c = Cursor::new(blk);
        let count = c.u32().ok_or_else(|| anyhow!("short bookkeeping block"))? as usize;
        c.u32(); // unknown word
        let mut offsets = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            offsets.push(c.u32().ok_or_else(|| anyhow!("short offset table"))?);
        }
        // The table is stored in 256-entry slabs; skip the unused tail of
        // the final slab before the directory.
        let slots = count.div_ceil(256) * 256;
        c.skip((slots - count) * 4);
        let num_dirs = c.u32().ok_or_else(|| anyhow!("short directory"))? as usize;
        let mut dir = HashMap::with_capacity(num_dirs);
        for _ in 0..num_dirs {
            let name_len = c.u8().ok_or_else(|| anyhow!("short directory entry"))? as usize;
            let name = c
                .ascii(name_len)
                .ok_or_else(|| anyhow!("short directory name"))?;
            let block_id = c.u32().ok_or_else(|| anyhow!("short directory block id"))?;
            dir.insert(name, block_id);
        }
        // Free lists follow; unused for read-only traversal.
        Ok(Self { data, offsets, dir })
    }

    /// Resolve a block id to its byte slice. The offset-table entry packs
    /// the 32-aligned address in its high bits and the size as a
    /// power-of-two exponent in its low 5 bits.
    fn block(&self, id: u32) -> Result<&'a [u8]> {
        let packed = *self
            .offsets
            .get(id as usize)
            .ok_or_else(|| anyhow!("block id {id} out of range"))?;
        let size = 1usize << (packed & 0x1f);
        let start = (packed & !0x1f) as usize + 4;
        self.data
            .get(start..start + size)
            .ok_or_else(|| anyhow!("block {id} out of range"))
    }
}

/// Recursively walk a B-tree node, appending records in key order. Any
/// malformed node or unknown record encoding flips `truncated` and stops
/// the walk — the records gathered so far stay.
///
/// `visited` rejects any node id seen before: a well-formed store's
/// B-tree is a tree, so a revisit means a crafted DAG / cycle. Without
/// it a shallow leveled DAG (every path under `MAX_DEPTH`) multiplies
/// visits exponentially while staying within the depth cap.
fn walk(
    alloc: &Allocator,
    node_id: u32,
    depth: usize,
    visited: &mut HashSet<u32>,
    store: &mut DsStore,
) {
    if store.truncated {
        return;
    }
    if depth > MAX_DEPTH || !visited.insert(node_id) {
        store.truncated = true;
        return;
    }
    let Ok(blk) = alloc.block(node_id) else {
        store.truncated = true;
        return;
    };
    let mut c = Cursor::new(blk);
    let (Some(next), Some(count)) = (c.u32(), c.u32()) else {
        store.truncated = true;
        return;
    };
    if next != 0 {
        for _ in 0..count {
            let Some(child) = c.u32() else {
                store.truncated = true;
                return;
            };
            walk(alloc, child, depth + 1, visited, store);
            if store.truncated {
                return;
            }
            match read_record(&mut c) {
                Some(r) => store.records.push(r),
                None => {
                    store.truncated = true;
                    return;
                }
            }
        }
        walk(alloc, next, depth + 1, visited, store);
    } else {
        for _ in 0..count {
            match read_record(&mut c) {
                Some(r) => store.records.push(r),
                None => {
                    store.truncated = true;
                    return;
                }
            }
        }
    }
}

/// Read one record at the cursor. `None` on a short buffer or an unknown
/// data-type tag (whose length we can't know, so the stream position is
/// no longer trustworthy).
fn read_record(c: &mut Cursor) -> Option<DsRecord> {
    let name_units = c.u32()? as usize;
    let name = c.utf16_be(name_units)?;
    let code = c.ascii(4)?;
    let data_type = c.take(4)?;
    let value = match data_type {
        b"long" | b"shor" => DsValue::Int(c.u32()?),
        b"bool" => DsValue::Bool(c.u8()? != 0),
        b"type" => DsValue::Type(c.ascii(4)?),
        b"comp" => DsValue::Long(c.u64()?),
        b"dutc" => DsValue::Date(c.u64()?),
        b"ustr" => {
            let units = c.u32()? as usize;
            DsValue::Str(c.utf16_be(units)?)
        }
        b"blob" => {
            let len = c.u32()? as usize;
            DsValue::Blob(c.take(len)?.to_vec())
        }
        _ => return None,
    };
    Some(DsRecord { name, code, value })
}

/// Sequential big-endian reader over a block's bytes.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    fn skip(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n);
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(be_u32)
    }

    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| {
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            u64::from_be_bytes(a)
        })
    }

    /// Read `n` bytes as an ASCII string (used for 4-char codes and
    /// directory names — all ASCII in practice). Bytes outside printable
    /// ASCII become `.` — these strings reach the terminal verbatim, so
    /// crafted bytes must not smuggle escape sequences into the output.
    fn ascii(&mut self, n: usize) -> Option<String> {
        self.take(n)
            .map(|b| b.iter().map(|&c| printable_ascii(c)).collect())
    }

    /// Read `units` UTF-16 code units (2 bytes each), big-endian.
    /// Control characters become U+FFFD for the same terminal-injection
    /// reason as [`Cursor::ascii`].
    fn utf16_be(&mut self, units: usize) -> Option<String> {
        let b = self.take(units.checked_mul(2)?)?;
        let u16s: Vec<u16> = b
            .chunks_exact(2)
            .map(|p| u16::from_be_bytes([p[0], p[1]]))
            .collect();
        Some(
            String::from_utf16_lossy(&u16s)
                .chars()
                .map(|c| if c.is_control() { '\u{fffd}' } else { c })
                .collect(),
        )
    }
}

fn be_u32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Map a raw byte to a displayable char: printable ASCII passes, anything
/// else (controls, DEL, high bytes that would alias to C1 controls)
/// becomes `.`.
pub(super) fn printable_ascii(c: u8) -> char {
    if (0x20..0x7f).contains(&c) {
        c as char
    } else {
        '.'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u32(buf: &mut [u8], off: usize, v: u32) {
        buf[off..off + 4].copy_from_slice(&v.to_be_bytes());
    }

    /// Write a `(name, Iloc, blob[x, y])` record at `pos`; return the
    /// position just past it.
    fn put_iloc_record(buf: &mut [u8], mut pos: usize, name: &str, x: u32, y: u32) -> usize {
        put_u32(buf, pos, name.encode_utf16().count() as u32); // name length (units)
        pos += 4;
        for ch in name.encode_utf16() {
            buf[pos..pos + 2].copy_from_slice(&ch.to_be_bytes());
            pos += 2;
        }
        buf[pos..pos + 4].copy_from_slice(b"Iloc");
        pos += 4;
        buf[pos..pos + 4].copy_from_slice(b"blob");
        pos += 4;
        put_u32(buf, pos, 16); // blob length
        pos += 4;
        put_u32(buf, pos, x);
        put_u32(buf, pos + 4, y);
        pos + 16
    }

    /// Build a minimal valid store: one leaf node holding a single
    /// `(a.txt, Iloc, blob[x=10,y=20])` record, a DSDB header pointing at
    /// it, and a bookkeeping block with a 3-entry offset table + the
    /// `DSDB` directory. Block addresses are 32-aligned so the low 5 bits
    /// carry the size class.
    fn synthetic_store() -> Vec<u8> {
        // Block layout (allocator address → file offset = addr + 4):
        //   id 1 = DSDB header @ addr 0x40 (file 0x44), size 32   → 0x45
        //   id 2 = leaf node   @ addr 0x80 (file 0x84), size 64   → 0x86
        //   id 0 = root block  @ addr 0x800 (file 0x804), size 2048 → 0x80b
        let mut buf = vec![0u8; 0x804 + 2048];

        // File header.
        put_u32(&mut buf, 0, 1);
        buf[4..8].copy_from_slice(b"Bud1");
        put_u32(&mut buf, 8, 0x800); // root offset
        put_u32(&mut buf, 12, 2048); // root size
        put_u32(&mut buf, 16, 0x800); // copy

        // DSDB header block @ file 0x44: rootNode=2, then levels/records/
        // nodes/pageSize (unused by the parser).
        put_u32(&mut buf, 0x44, 2);
        put_u32(&mut buf, 0x44 + 4, 0);
        put_u32(&mut buf, 0x44 + 8, 1);
        put_u32(&mut buf, 0x44 + 12, 1);
        put_u32(&mut buf, 0x44 + 16, 4096);

        // Leaf node @ file 0x84: next=0, count=1, then the record.
        let leaf = 0x84;
        put_u32(&mut buf, leaf, 0); // next (leaf)
        put_u32(&mut buf, leaf + 4, 1); // count
        put_iloc_record(&mut buf, leaf + 8, "a.txt", 10, 20);

        // Bookkeeping block @ file 0x804.
        let root = 0x804;
        put_u32(&mut buf, root, 3); // offset count
        put_u32(&mut buf, root + 4, 0); // unknown
        put_u32(&mut buf, root + 8, 0x80b); // id 0 → root block
        put_u32(&mut buf, root + 12, 0x45); // id 1 → DSDB header
        put_u32(&mut buf, root + 16, 0x86); // id 2 → leaf node
        // Offset table reserves 256 slots; the directory follows at +8 +
        // 256*4.
        let dir = root + 8 + 256 * 4;
        put_u32(&mut buf, dir, 1); // one directory entry
        buf[dir + 4] = 4; // name length
        buf[dir + 5..dir + 9].copy_from_slice(b"DSDB");
        put_u32(&mut buf, dir + 9, 1); // → block id 1

        buf
    }

    /// Build a two-level store: one internal node over two leaves, with a
    /// separator record held in the internal node itself. Exercises the
    /// `next != 0` branch — in-order interleaving of child + separator
    /// record, then the rightmost `next` child. Expected key order:
    /// `a.txt` (left leaf), `m.txt` (separator), `z.txt` (right leaf).
    fn internal_node_store() -> Vec<u8> {
        // Block layout (allocator address → file offset = addr + 4):
        //   id 1 = DSDB header   @ addr 0x40  (file 0x44),  size 32   → 0x45
        //   id 2 = internal node @ addr 0x80  (file 0x84),  size 64   → 0x86
        //   id 3 = left leaf     @ addr 0xC0  (file 0xC4),  size 64   → 0xC6
        //   id 4 = right leaf    @ addr 0x100 (file 0x104), size 64   → 0x106
        //   id 0 = root block    @ addr 0x800 (file 0x804), size 2048 → 0x80b
        let mut buf = vec![0u8; 0x804 + 2048];

        // File header.
        put_u32(&mut buf, 0, 1);
        buf[4..8].copy_from_slice(b"Bud1");
        put_u32(&mut buf, 8, 0x800);
        put_u32(&mut buf, 12, 2048);
        put_u32(&mut buf, 16, 0x800);

        // DSDB header: rootNode = 2 (the internal node).
        put_u32(&mut buf, 0x44, 2);
        put_u32(&mut buf, 0x44 + 16, 4096);

        // Internal node @ file 0x84: next = 4 (rightmost child), count = 1,
        // then childId = 3 (left child) followed by the separator record.
        let internal = 0x84;
        put_u32(&mut buf, internal, 4); // next (rightmost child id)
        put_u32(&mut buf, internal + 4, 1); // count
        put_u32(&mut buf, internal + 8, 3); // child id before the separator
        put_iloc_record(&mut buf, internal + 12, "m.txt", 30, 40);

        // Left leaf @ file 0xC4.
        put_u32(&mut buf, 0xC4, 0);
        put_u32(&mut buf, 0xC4 + 4, 1);
        put_iloc_record(&mut buf, 0xC4 + 8, "a.txt", 10, 20);

        // Right leaf @ file 0x104.
        put_u32(&mut buf, 0x104, 0);
        put_u32(&mut buf, 0x104 + 4, 1);
        put_iloc_record(&mut buf, 0x104 + 8, "z.txt", 50, 60);

        // Bookkeeping block: 5-entry offset table + the DSDB directory.
        let root = 0x804;
        put_u32(&mut buf, root, 5); // offset count
        put_u32(&mut buf, root + 8, 0x80b); // id 0 → root block
        put_u32(&mut buf, root + 12, 0x45); // id 1 → DSDB header
        put_u32(&mut buf, root + 16, 0x86); // id 2 → internal node
        put_u32(&mut buf, root + 20, 0xC6); // id 3 → left leaf
        put_u32(&mut buf, root + 24, 0x106); // id 4 → right leaf
        let dir = root + 8 + 256 * 4;
        put_u32(&mut buf, dir, 1);
        buf[dir + 4] = 4;
        buf[dir + 5..dir + 9].copy_from_slice(b"DSDB");
        put_u32(&mut buf, dir + 9, 1); // → block id 1

        buf
    }

    #[test]
    fn parses_single_iloc_record() {
        let buf = synthetic_store();
        let store = parse(&buf).expect("parses");
        assert!(!store.truncated);
        assert_eq!(store.records.len(), 1);
        let r = &store.records[0];
        assert_eq!(r.name, "a.txt");
        assert_eq!(r.code, "Iloc");
        match &r.value {
            DsValue::Blob(b) => {
                assert_eq!(be_u32(&b[0..4]), 10);
                assert_eq!(be_u32(&b[4..8]), 20);
            }
            _ => panic!("expected blob"),
        }
    }

    #[test]
    fn parses_internal_node_in_key_order() {
        let buf = internal_node_store();
        let store = parse(&buf).expect("parses");
        assert!(!store.truncated);
        let names: Vec<&str> = store.records.iter().map(|r| r.name.as_str()).collect();
        // Left-child records, then the in-node separator, then the
        // rightmost child — strict B-tree key order.
        assert_eq!(names, ["a.txt", "m.txt", "z.txt"]);
    }

    #[test]
    fn rejects_non_bud1() {
        let mut buf = vec![0u8; 64];
        buf[4..8].copy_from_slice(b"junk");
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_too_small() {
        assert!(parse(b"\x00\x00\x00\x01Bud1").is_err());
    }

    #[test]
    fn leveled_dag_marks_truncated_not_hang() {
        // A crafted shallow DAG: the internal node's separator child and
        // its rightmost `next` child are the SAME leaf. Every path stays
        // far under MAX_DEPTH, so only the visited-set guard stops the
        // revisit (and, scaled up, the exponential blow-up).
        let mut buf = internal_node_store();
        // Internal node @ file 0x84: next was 4 (right leaf); point it at
        // the left leaf (id 3) already reached via the child slot.
        put_u32(&mut buf, 0x84, 3);
        let store = parse(&buf).expect("header still parses");
        assert!(store.truncated);
        // First traversal of the shared leaf + the separator survived.
        let names: Vec<&str> = store.records.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["a.txt", "m.txt"]);
    }

    #[test]
    fn self_loop_node_marks_truncated_not_hang() {
        // Internal node whose rightmost `next` child is itself.
        let mut buf = internal_node_store();
        put_u32(&mut buf, 0x84, 2); // next → node 2 (itself)
        let store = parse(&buf).expect("header still parses");
        assert!(store.truncated);
    }

    /// Record names and codes are attacker bytes that land verbatim in
    /// the rendered table — an embedded ESC must never survive parsing.
    #[test]
    fn control_bytes_in_name_and_code_are_sanitised() {
        let mut buf = synthetic_store();
        let leaf = 0x84;
        // Record layout at leaf+8: nameLen(4) │ name UTF-16 (5 units) │
        // code(4). Turn the name's first unit ("a") into ESC, and the
        // code's first byte ("I" of Iloc) into a raw ESC byte.
        buf[leaf + 12] = 0x00;
        buf[leaf + 13] = 0x1b;
        buf[leaf + 22] = 0x1b;
        let store = parse(&buf).expect("parses");
        let r = &store.records[0];
        assert_eq!(r.name, "\u{fffd}.txt");
        assert_eq!(r.code, ".loc");
    }

    #[test]
    fn out_of_range_block_marks_truncated_not_panic() {
        // A root node id past the offset table must surface as truncated,
        // never panic.
        let mut buf = synthetic_store();
        // Point the DSDB rootNode at a non-existent block id.
        put_u32(&mut buf, 0x44, 99);
        let store = parse(&buf).expect("header still parses");
        assert!(store.truncated);
        assert!(store.records.is_empty());
    }
}
