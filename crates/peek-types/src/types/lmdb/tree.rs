//! B+tree traversal over an [`LmdbEnv`]: an in-order leaf walker that
//! reads only branch pages, plus expansion of leaf nodes into rows (one
//! row per `(key, value)` pair, so a dupsort key yields one row per
//! duplicate) and key lookup.
//!
//! Hostile-input bounds: descent depth is capped at [`MAX_DEPTH`] (a cycle
//! along one path deepens forever) and a walk may visit at most as many
//! pages as the file holds (a DAG of shared subtrees would otherwise
//! revisit pages exponentially). Both hold in O(1) memory.

use anyhow::{Result, bail, ensure};
use bytes::Bytes;

use super::env::LmdbEnv;
use super::format::{
    DbRecord, F_BIGDATA, F_DUPDATA, F_SUBDATA, Node, Page, PageHeader, cmp_keys, le64,
};

/// LMDB trees are a handful of levels deep; anything past this is a cycle
/// or a crafted file.
const MAX_DEPTH: usize = 32;

/// A leaf page reached by [`LeafWalker`], with its header.
pub struct LeafRef {
    pub pgno: u64,
    pub hdr: PageHeader,
}

/// In-order walk over a tree's leaf pages. Holds the branch path; leaves
/// are reported by header only, so counting them stays cheap.
pub struct LeafWalker {
    pending_root: Option<u64>,
    stack: Vec<(Page, usize)>,
    pages_visited: u64,
}

impl LeafWalker {
    pub fn new(root: Option<u64>) -> Self {
        Self {
            pending_root: root,
            stack: Vec::new(),
            pages_visited: 0,
        }
    }

    pub fn next_leaf(&mut self, env: &LmdbEnv) -> Result<Option<LeafRef>> {
        if let Some(root) = self.pending_root.take() {
            return self.descend(env, root).map(Some);
        }
        loop {
            let Some((page, idx)) = self.stack.last_mut() else {
                return Ok(None);
            };
            if *idx >= page.num_keys() {
                self.stack.pop();
                continue;
            }
            let child = page.node(*idx)?.child_pgno();
            *idx += 1;
            return self.descend(env, child).map(Some);
        }
    }

    /// Follow first children from `pgno` down to a leaf.
    fn descend(&mut self, env: &LmdbEnv, mut pgno: u64) -> Result<LeafRef> {
        loop {
            self.pages_visited += 1;
            ensure!(
                self.pages_visited <= env.page_count(),
                "tree walk visited more pages than the file holds"
            );
            ensure!(self.stack.len() < MAX_DEPTH, "tree deeper than {MAX_DEPTH}");
            let hdr = env.read_header(pgno)?;
            if hdr.is_leaf() {
                return Ok(LeafRef { pgno, hdr });
            }
            if !hdr.is_branch() {
                bail!(
                    "page {pgno}: expected branch or leaf, flags {:#x}",
                    hdr.flags
                );
            }
            let page = env.read_page(pgno)?;
            ensure!(page.num_keys() > 0, "page {pgno}: empty branch");
            let first = page.node(0)?.child_pgno();
            self.stack.push((page, 1));
            pgno = first;
        }
    }
}

/// Where a row's value bytes live.
#[derive(Clone)]
pub enum ValueRef {
    Inline(Bytes),
    /// `len` bytes starting at overflow page `pgno`.
    Overflow {
        pgno: u64,
        len: u64,
    },
    /// A named-DB record (MAIN rows only).
    Database(DbRecord),
}

impl ValueRef {
    pub fn size(&self) -> u64 {
        match self {
            ValueRef::Inline(b) => b.len() as u64,
            ValueRef::Overflow { len, .. } => *len,
            ValueRef::Database(_) => 0,
        }
    }
}

/// One `(key, value)` row. `dup` is the value's index among its key's
/// duplicates, `Some` only in a dupsort DB.
pub struct Item {
    pub key: Bytes,
    pub value: ValueRef,
    pub dup: Option<u64>,
}

/// Rows a leaf page contributes. A plain leaf counts its nodes from the
/// header alone; a dupsort leaf needs the page to count each key's dups.
pub fn leaf_rows(env: &LmdbEnv, db: &DbRecord, leaf: &LeafRef) -> Result<u64> {
    if !db.is_dupsort() {
        return Ok(leaf.hdr.num_keys() as u64);
    }
    let page = env.read_page(leaf.pgno)?;
    let mut rows = 0u64;
    for i in 0..page.num_keys() {
        rows = rows.saturating_add(node_rows(env, &page.node(i)?)?);
    }
    Ok(rows)
}

/// Rows one leaf node expands to: 1, or its duplicate count.
fn node_rows(env: &LmdbEnv, node: &Node) -> Result<u64> {
    if node.flags & F_DUPDATA == 0 {
        return Ok(1);
    }
    let data = node.data()?;
    if node.flags & F_SUBDATA != 0 {
        let sub = DbRecord::parse(&data).ok_or_else(|| anyhow::anyhow!("bad dup sub-DB"))?;
        Ok(sub.entries)
    } else {
        Ok(Page::new(data, env.variant)?.num_keys() as u64)
    }
}

/// Append up to `take` rows of leaf `page` to `out`, skipping the first
/// `skip`. Returns how many of `skip` remain (when the page had fewer).
pub fn expand_leaf(
    env: &LmdbEnv,
    db: &DbRecord,
    page: &Page,
    mut skip: u64,
    take: usize,
    out: &mut Vec<Item>,
) -> Result<u64> {
    for i in 0..page.num_keys() {
        if out.len() >= take {
            break;
        }
        let node = page.node(i)?;
        if db.is_dupsort() && node.flags & F_DUPDATA != 0 {
            let n = node_rows(env, &node)?;
            if skip >= n {
                skip -= n;
                continue;
            }
            expand_dups(env, &node, skip, take, out)?;
            skip = 0;
            continue;
        }
        if skip > 0 {
            skip -= 1;
            continue;
        }
        out.push(Item {
            value: plain_value(&node)?,
            dup: db.is_dupsort().then_some(0),
            key: node.key,
        });
    }
    Ok(skip)
}

/// Value of a non-dup leaf node.
fn plain_value(node: &Node) -> Result<ValueRef> {
    let data = node.data()?;
    if node.flags & F_BIGDATA != 0 {
        let pgno = le64(&data, 0).ok_or_else(|| anyhow::anyhow!("bad overflow pgno"))?;
        return Ok(ValueRef::Overflow {
            pgno,
            len: node.data_size(),
        });
    }
    if node.flags & F_SUBDATA != 0
        && let Some(db) = DbRecord::parse(&data)
    {
        return Ok(ValueRef::Database(db));
    }
    Ok(ValueRef::Inline(data))
}

/// Push a dupsort node's duplicates (from `skip`) as rows. Dups are the
/// keys of an inline sub-page or of a sub-tree. A DUPFIXED dup's size
/// lives in the sub-page's `pad` or the sub-DB record's `pad`.
fn expand_dups(
    env: &LmdbEnv,
    node: &Node,
    skip: u64,
    take: usize,
    out: &mut Vec<Item>,
) -> Result<()> {
    let data = node.data()?;
    let mut idx = skip;
    let mut push = |value: Bytes, out: &mut Vec<Item>| {
        out.push(Item {
            key: node.key.clone(),
            value: ValueRef::Inline(value),
            dup: Some(idx),
        });
        idx += 1;
    };
    if node.flags & F_SUBDATA == 0 {
        let sub = Page::new(data, env.variant)?;
        let ksize = usize::from(sub.hdr.pad);
        for i in (skip as usize)..sub.num_keys() {
            if out.len() >= take {
                break;
            }
            push(dup_key(&sub, i, ksize)?, out);
        }
        return Ok(());
    }
    let sub = DbRecord::parse(&data).ok_or_else(|| anyhow::anyhow!("bad dup sub-DB"))?;
    let mut walker = LeafWalker::new(sub.root);
    let mut skip = skip;
    while out.len() < take
        && let Some(leaf) = walker.next_leaf(env)?
    {
        let n = leaf.hdr.num_keys() as u64;
        if skip >= n {
            skip -= n;
            continue;
        }
        let page = env.read_page(leaf.pgno)?;
        for i in (skip as usize)..page.num_keys() {
            if out.len() >= take {
                break;
            }
            push(dup_key(&page, i, sub.pad as usize)?, out);
        }
        skip = 0;
    }
    Ok(())
}

/// Key `i` of a dup page: packed fixed-size on LEAF2, else a node key.
fn dup_key(page: &Page, i: usize, ksize: usize) -> Result<Bytes> {
    if page.hdr.is_leaf2() {
        page.leaf2_key(i, ksize)
    } else {
        Ok(page.node(i)?.key)
    }
}

/// Leaf node whose key equals `key`. Descends by the DB's built-in key
/// order; when that misses (an app-defined comparator), falls back to a
/// full in-order scan.
pub fn find(env: &LmdbEnv, db: &DbRecord, key: &[u8]) -> Result<Option<Node>> {
    find_any(env, db, &[key])
}

/// Leaf node matching any of `keys`. Tries a descent per key, in order,
/// before one shared full scan — so a miss on the first spelling costs a
/// descent, not a walk of the whole tree.
pub fn find_any(env: &LmdbEnv, db: &DbRecord, keys: &[&[u8]]) -> Result<Option<Node>> {
    let Some(root) = db.root else {
        return Ok(None);
    };
    for key in keys {
        if let Some(node) = descend_to(env, db, root, key)? {
            return Ok(Some(node));
        }
    }
    let mut walker = LeafWalker::new(db.root);
    while let Some(leaf) = walker.next_leaf(env)? {
        let page = env.read_page(leaf.pgno)?;
        for key in keys {
            if let Some(node) = scan_leaf(&page, key)? {
                return Ok(Some(node));
            }
        }
    }
    Ok(None)
}

fn descend_to(env: &LmdbEnv, db: &DbRecord, mut pgno: u64, key: &[u8]) -> Result<Option<Node>> {
    for _ in 0..MAX_DEPTH {
        let page = env.read_page(pgno)?;
        if page.hdr.is_leaf() {
            return scan_leaf(&page, key);
        }
        ensure!(page.hdr.is_branch(), "page {pgno}: not a branch or leaf");
        // Node 0's key is implicit (-∞): take the last child whose key ≤ target.
        let mut child = page.node(0)?.child_pgno();
        for i in 1..page.num_keys() {
            let node = page.node(i)?;
            if cmp_keys(db.flags, &node.key, key).is_gt() {
                break;
            }
            child = node.child_pgno();
        }
        pgno = child;
    }
    bail!("tree deeper than {MAX_DEPTH}")
}

fn scan_leaf(page: &Page, key: &[u8]) -> Result<Option<Node>> {
    for i in 0..page.num_keys() {
        let node = page.node(i)?;
        if node.key == key {
            return Ok(Some(node));
        }
    }
    Ok(None)
}

/// Value of row `dup` (or the single value) under a found leaf node.
pub fn node_value(env: &LmdbEnv, db: &DbRecord, node: &Node, dup: u64) -> Result<Option<ValueRef>> {
    if db.is_dupsort() && node.flags & F_DUPDATA != 0 {
        let mut out = Vec::with_capacity(1);
        expand_dups(env, node, dup, 1, &mut out)?;
        return Ok(out.pop().map(|item| item.value));
    }
    (dup == 0).then(|| plain_value(node)).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::lmdb::format::tests::leaf2_page_bytes;
    use crate::types::lmdb::format::{MDB_DUPFIXED, MDB_DUPSORT, Variant};
    use crate::types::lmdb::testing::{catalogs, event_log};

    /// Every row of `db`, in order.
    fn all_rows(env: &LmdbEnv, db: &DbRecord) -> Vec<Item> {
        let mut out = Vec::new();
        let mut walker = LeafWalker::new(db.root);
        while let Some(leaf) = walker.next_leaf(env).unwrap() {
            let page = env.read_page(leaf.pgno).unwrap();
            expand_leaf(env, db, &page, 0, usize::MAX, &mut out).unwrap();
        }
        out
    }

    fn named(env: &LmdbEnv, name: &[u8]) -> DbRecord {
        let dbs = env.named_dbs().unwrap().dbs;
        dbs.into_iter().find(|d| &d.name[..] == name).unwrap().db
    }

    #[test]
    fn row_counts_match_mdb_stat() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source.clone()).unwrap();
            for (name, n) in [(&b"users"[..], 600), (b"tags", 30), (b"blobs", 4)] {
                let db = named(&env, name);
                assert_eq!(all_rows(&env, &db).len() as u64, n);
                assert_eq!(db.entries, n);
            }
            let env = LmdbEnv::open(&event_log()).unwrap();
            assert_eq!(all_rows(&env, &env.meta.main).len(), 24);
        }
    }

    #[test]
    fn rows_are_in_key_order_with_dup_indexes() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source.clone()).unwrap();
            let users = all_rows(&env, &named(&env, b"users"));
            assert_eq!(&users[0].key[..], b"user:00001");
            assert_eq!(&users[599].key[..], b"user:00600");

            let tags = all_rows(&env, &named(&env, b"tags"));
            assert_eq!(&tags[0].key[..], b"archived");
            let dups: Vec<u64> = tags[..6].iter().map(|t| t.dup.unwrap()).collect();
            assert_eq!(dups, [0, 1, 2, 3, 4, 5]);
            assert_eq!(&tags[6].key[..], b"beta");
        }
    }

    #[test]
    fn overflow_value_reads_back() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source.clone()).unwrap();
            let blobs = all_rows(&env, &named(&env, b"blobs"));
            let big = blobs.iter().find(|b| b.value.size() == 40_000).unwrap();
            let ValueRef::Overflow { pgno, len } = big.value else {
                panic!("40 KB value should spill to overflow pages");
            };
            let prefix = env.read_overflow_prefix(pgno, len, 16).unwrap();
            assert_eq!(prefix.len(), 16);
            assert!(env.overflow_offset(pgno, len).unwrap() + len <= env.file_len);
        }
    }

    #[test]
    fn main_lists_named_db_records() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source.clone()).unwrap();
            let main = all_rows(&env, &env.meta.main);
            let dbs = main
                .iter()
                .filter(|r| matches!(r.value, ValueRef::Database(_)))
                .count();
            assert_eq!((main.len(), dbs), (6, 4));
        }
    }

    #[test]
    fn skip_lands_on_the_right_row_across_leaves() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source.clone()).unwrap();
            let db = named(&env, b"users");
            let all = all_rows(&env, &db);
            // Skip into the middle via the walker, as the row set does.
            let mut walker = LeafWalker::new(db.root);
            let mut skip = 450u64;
            let mut out = Vec::new();
            while let Some(leaf) = walker.next_leaf(&env).unwrap() {
                let n = leaf_rows(&env, &db, &leaf).unwrap();
                if skip >= n {
                    skip -= n;
                    continue;
                }
                let page = env.read_page(leaf.pgno).unwrap();
                skip = expand_leaf(&env, &db, &page, skip, 3, &mut out).unwrap();
                if out.len() >= 3 {
                    break;
                }
            }
            assert_eq!(out[0].key, all[450].key);
        }
    }

    #[test]
    fn find_by_key_including_integer_keys_and_dups() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source.clone()).unwrap();
            let users = named(&env, b"users");
            let node = find(&env, &users, b"user:00321").unwrap().unwrap();
            let ValueRef::Inline(v) = node_value(&env, &users, &node, 0).unwrap().unwrap() else {
                panic!("inline JSON value");
            };
            assert!(v.starts_with(b"{\"id\":321,"));
            assert!(find(&env, &users, b"user:99999").unwrap().is_none());

            let tags = named(&env, b"tags");
            let node = find(&env, &tags, b"legacy").unwrap().unwrap();
            assert!(node_value(&env, &tags, &node, 5).unwrap().is_some());
            assert!(node_value(&env, &tags, &node, 6).unwrap().is_none());

            let env = LmdbEnv::open(&event_log()).unwrap();
            let key = (1_760_000_000u64 + 5 * 3600).to_le_bytes();
            assert!(find(&env, &env.meta.main, &key).unwrap().is_some());
        }
    }

    /// A leaf page with one `F_DUPDATA` node `key` whose dups sit in an
    /// inline LEAF2 sub-page of `n` 4-byte values, laid out per `v`.
    fn dupfixed_leaf(v: Variant, key: &[u8], n: usize) -> Page {
        assert!(key.len().is_multiple_of(2), "even key: no 1.0 data padding");
        let sub = leaf2_page_bytes(v, 4, n);
        let h = v.header;
        let node = h + 8;
        let mut page = vec![0u8; node + 8 + key.len() + sub.len()];
        page[h - 6..h - 4].copy_from_slice(&0x02u16.to_le_bytes()); // P_LEAF
        page[h - 4..h - 2].copy_from_slice(&((h + 2 - v.base) as u16).to_le_bytes()); // lower
        page[h..h + 2].copy_from_slice(&((node - v.base) as u16).to_le_bytes());
        page[node..node + 2].copy_from_slice(&(sub.len() as u16).to_le_bytes()); // dsize
        page[node + 4..node + 6].copy_from_slice(&F_DUPDATA.to_le_bytes());
        page[node + 6..node + 8].copy_from_slice(&(key.len() as u16).to_le_bytes());
        page[node + 8..node + 8 + key.len()].copy_from_slice(key);
        page[node + 8 + key.len()..].copy_from_slice(&sub);
        Page::new(Bytes::from(page), v).unwrap()
    }

    #[test]
    fn dupfixed_sub_page_rows_use_its_key_size() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source).unwrap();
            let page = dupfixed_leaf(env.variant, b"k1", 3);
            // A stray top-level `pad` must not change the dup size.
            let db = DbRecord {
                pad: 99,
                flags: MDB_DUPSORT | MDB_DUPFIXED,
                depth: 1,
                branch_pages: 0,
                leaf_pages: 1,
                overflow_pages: 0,
                entries: 3,
                root: None,
            };
            let mut out = Vec::new();
            expand_leaf(&env, &db, &page, 0, usize::MAX, &mut out).unwrap();
            assert_eq!(out.len(), 3, "{:?}", env.variant);
            for (i, item) in out.iter().enumerate() {
                assert_eq!(&item.key[..], b"k1");
                assert_eq!(item.dup, Some(i as u64));
                let ValueRef::Inline(v) = &item.value else {
                    panic!("dup values are inline");
                };
                assert_eq!(&v[..], &[i as u8; 4]);
            }

            let mut out = Vec::new();
            assert_eq!(expand_leaf(&env, &db, &page, 1, 1, &mut out).unwrap(), 0);
            assert_eq!(out[0].dup, Some(1));

            let node = page.node(0).unwrap();
            let Some(ValueRef::Inline(v)) = node_value(&env, &db, &node, 2).unwrap() else {
                panic!("dup 2 exists");
            };
            assert_eq!(&v[..], &[2u8; 4]);
            assert!(node_value(&env, &db, &node, 3).unwrap().is_none());
        }
    }

    #[test]
    fn find_any_tries_each_spelling() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source).unwrap();
            let main = env.meta.main;
            // 1.0 stores DB names with a trailing NUL, 0.9 without.
            let node = find_any(&env, &main, &[b"nosuch", b"users\0", b"users"])
                .unwrap()
                .unwrap();
            assert!(node.key.starts_with(b"users"));
            assert!(
                find_any(&env, &main, &[b"nosuch", b"nosuch\0"])
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn cyclic_branch_stops_cleanly() {
        use crate::types::lmdb::testing::{CATALOG, CATALOG_09};
        for path in [CATALOG, CATALOG_09] {
            // Point the `users` root branch's first child back at itself.
            let bytes = std::fs::read(path).unwrap();
            let env = LmdbEnv::open(&InputSource::memory(bytes.clone(), "c")).unwrap();
            let root = named(&env, b"users").root.unwrap();
            let page_start = (root * env.psize) as usize;
            let v = env.variant;
            let ptr = u16::from_le_bytes([
                bytes[page_start + v.header],
                bytes[page_start + v.header + 1],
            ]);
            let node = page_start + v.base + usize::from(ptr);
            let mut bad = bytes;
            bad[node..node + 2].copy_from_slice(&(root as u16).to_le_bytes());
            bad[node + 2..node + 6].fill(0);

            let env = LmdbEnv::open(&InputSource::memory(bad, "c")).unwrap();
            let db = named(&env, b"users");
            let mut walker = LeafWalker::new(db.root);
            let result = (|| -> Result<()> {
                while walker.next_leaf(&env)?.is_some() {}
                Ok(())
            })();
            assert!(result.is_err(), "cycle must surface as an error, not hang");
        }
    }

    use peek_io::InputSource;
}
