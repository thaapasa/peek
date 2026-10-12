//! [`RowSource`] over one LMDB database: `Key │ Value │ Size`, one row per
//! `(key, value)` pair.
//!
//! The row total comes straight from the DB record's `entries`, so it's
//! definite without a walk. Random access goes through a lazily grown
//! **leaf index** — `(pgno, first_row)` per leaf, appended as the walker
//! passes each one. A plain leaf costs a 16-byte header read to count; a
//! dupsort leaf is read whole to sum its duplicates. Rows themselves load
//! as a sliding window ([`WINDOW_SIZE`]) centred on the requested row, like
//! `SqliteRowSet`. Value cells preview at most [`PREVIEW_BYTES`], so a
//! gigabyte overflow value costs one small read.

use anyhow::Result;
use peek_io::InputSource;

use super::cells::{self, PREVIEW_BYTES};
use super::env::LmdbEnv;
use super::format::DbRecord;
use super::tree::{self, Item, LeafWalker, ValueRef};
use crate::viewer::modes::ExtractTarget;
use crate::viewer::table::WINDOW_SIZE;
use crate::viewer::table::row_source::RowSource;
use crate::viewer::table::rows_mode::Alignment;

const HEADER: [&str; 3] = ["Key", "Value", "Size"];

struct Row {
    cells: Vec<Option<String>>,
    /// Inner path for extract; `None` for named-DB rows in MAIN.
    path: Option<String>,
    size: u64,
}

pub struct LmdbRowSet {
    env: LmdbEnv,
    db: DbRecord,
    /// Inner-path DB segment for this DB's rows.
    db_seg: String,
    walker: LeafWalker,
    /// `(pgno, first body row)` per leaf, in tree order.
    leaves: Vec<(u64, u64)>,
    /// Body rows covered by `leaves`.
    indexed_rows: u64,
    walk_done: bool,
    /// Records including the synthetic header row.
    total: usize,
    /// Record index of `window[0]`.
    window_start: usize,
    window: Vec<Row>,
    header: Vec<Option<String>>,
}

impl LmdbRowSet {
    /// Rows of `db`; `name` is its MAIN key (`None` for MAIN itself).
    pub fn open(source: &InputSource, db: DbRecord, name: Option<&[u8]>) -> Result<Self> {
        let env = LmdbEnv::open(source)?;
        let total = usize::try_from(db.entries)
            .unwrap_or(usize::MAX - 1)
            .saturating_add(1);
        let mut rows = Self {
            env,
            db,
            db_seg: cells::db_segment(name),
            walker: LeafWalker::new(db.root),
            leaves: Vec::new(),
            indexed_rows: 0,
            walk_done: db.root.is_none(),
            total,
            window_start: 1,
            window: Vec::new(),
            header: HEADER.iter().map(|h| Some((*h).to_string())).collect(),
        };
        // Prime the first window so the table seeds its column widths from
        // real rows, not just the header.
        rows.ensure_row(1)?;
        Ok(rows)
    }

    pub fn alignments() -> Vec<Alignment> {
        vec![Alignment::Left, Alignment::Left, Alignment::Right]
    }

    /// Grow the leaf index until it covers body row `row` or the tree ends.
    fn index_through(&mut self, row: u64) -> Result<()> {
        while !self.walk_done && self.indexed_rows <= row {
            match self.walker.next_leaf(&self.env)? {
                Some(leaf) => {
                    let n = tree::leaf_rows(&self.env, &self.db, &leaf)?;
                    self.leaves.push((leaf.pgno, self.indexed_rows));
                    self.indexed_rows = self.indexed_rows.saturating_add(n);
                }
                None => self.walk_done = true,
            }
        }
        Ok(())
    }

    /// Load up to `count` body rows starting at body row `start`.
    fn load(&mut self, start: u64, count: usize) -> Result<Vec<Item>> {
        self.index_through(start)?;
        let mut out = Vec::with_capacity(count.min(WINDOW_SIZE));
        let mut li = self.leaves.partition_point(|&(_, first)| first <= start);
        if li == 0 {
            return Ok(out);
        }
        li -= 1;
        let mut skip = start - self.leaves[li].1;
        while out.len() < count {
            if li == self.leaves.len() {
                self.index_through(self.indexed_rows)?;
                if li == self.leaves.len() {
                    break;
                }
            }
            let page = self.env.read_page(self.leaves[li].0)?;
            skip = tree::expand_leaf(&self.env, &self.db, &page, skip, count, &mut out)?;
            li += 1;
        }
        Ok(out)
    }

    fn to_row(&self, item: Item) -> Row {
        let size = item.value.size();
        let value = match &item.value {
            ValueRef::Inline(b) => {
                let shown = &b[..b.len().min(PREVIEW_BYTES)];
                cells::value_cell(shown, size, self.db.flags, item.dup.is_some())
            }
            ValueRef::Overflow { pgno, len } => {
                match self.env.read_overflow_prefix(*pgno, *len, PREVIEW_BYTES) {
                    Ok(b) => cells::value_cell(&b, size, self.db.flags, false),
                    Err(e) => format!("<unreadable: {e}>"),
                }
            }
            ValueRef::Database(db) => format!("<database: {} entries>", db.entries),
        };
        let path = (!matches!(item.value, ValueRef::Database(_)))
            .then(|| cells::row_path(&self.db_seg, &item.key, item.dup));
        Row {
            cells: vec![
                Some(cells::key_cell(&item.key, self.db.flags)),
                Some(value),
                Some(size.to_string()),
            ],
            path,
            size,
        }
    }

    fn row_at(&self, idx: usize) -> Option<&Row> {
        self.window.get(idx.checked_sub(self.window_start)?)
    }
}

impl RowSource for LmdbRowSet {
    fn ensure_row(&mut self, idx: usize) -> Result<usize> {
        if idx == 0 || idx >= self.total || self.row_at(idx).is_some() {
            return Ok(self.total);
        }
        let start = idx.saturating_sub(WINDOW_SIZE / 2).max(1);
        let items = self.load(start as u64 - 1, WINDOW_SIZE)?;
        self.window = items.into_iter().map(|item| self.to_row(item)).collect();
        self.window_start = start;
        Ok(self.total)
    }

    fn ensure_all(&mut self) -> Result<()> {
        Ok(())
    }

    fn row(&self, idx: usize) -> Option<&[Option<String>]> {
        if idx == 0 {
            return Some(&self.header);
        }
        self.row_at(idx).map(|r| r.cells.as_slice())
    }

    fn loaded(&self) -> usize {
        self.total
    }

    fn total(&self) -> Option<usize> {
        Some(self.total)
    }

    fn column_count(&self) -> usize {
        HEADER.len()
    }

    fn extract_target(&self, idx: usize) -> Option<ExtractTarget> {
        let path = self.row_at(idx)?.path.clone()?;
        Some(ExtractTarget::EntryPath(path))
    }

    fn extract_size(&self, idx: usize) -> Option<u64> {
        self.row_at(idx).map(|r| r.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::lmdb::testing::{catalogs, event_log};

    fn named(source: &InputSource, name: &[u8]) -> LmdbRowSet {
        let env = LmdbEnv::open(source).unwrap();
        let db = env
            .named_dbs()
            .unwrap()
            .dbs
            .into_iter()
            .find(|d| &d.name[..] == name)
            .unwrap()
            .db;
        LmdbRowSet::open(source, db, Some(name)).unwrap()
    }

    fn cell(rs: &LmdbRowSet, idx: usize, col: usize) -> String {
        rs.row(idx).unwrap()[col].clone().unwrap()
    }

    #[test]
    fn header_and_total_from_db_record() {
        for source in catalogs() {
            let rs = named(&source.clone(), b"users");
            assert_eq!(rs.total(), Some(601));
            assert_eq!(cell(&rs, 0, 0), "Key");
        }
    }

    #[test]
    fn random_access_far_from_window() {
        for source in catalogs() {
            let mut rs = named(&source.clone(), b"users");
            rs.ensure_row(600).unwrap();
            assert_eq!(cell(&rs, 600, 0), "user:00600");
            rs.ensure_row(1).unwrap();
            assert_eq!(cell(&rs, 1, 0), "user:00001");
            assert!(cell(&rs, 1, 1).starts_with("{\"id\":1,"));
        }
    }

    #[test]
    fn dupsort_rows_flatten_with_dup_paths() {
        for source in catalogs() {
            let mut rs = named(&source.clone(), b"tags");
            assert_eq!(rs.total(), Some(31));
            rs.ensure_row(7).unwrap();
            assert_eq!(cell(&rs, 7, 0), "beta");
            let Some(ExtractTarget::EntryPath(p)) = rs.extract_target(7) else {
                panic!("dup row extractable");
            };
            assert_eq!(p, "tags/beta/0");
        }
    }

    #[test]
    fn binary_and_overflow_values_preview() {
        for source in catalogs() {
            let mut rs = named(&source.clone(), b"blobs");
            rs.ensure_row(1).unwrap();
            assert_eq!(cell(&rs, 1, 0), "00 00 00 01");
            assert!(cell(&rs, 1, 1).starts_with("00 01 02 03"));
            let big = (1..=4).find(|&i| cell(&rs, i, 2) == "40000").unwrap();
            assert!(cell(&rs, big, 1).ends_with('…'));
            assert_eq!(rs.extract_size(big), Some(40_000));
            let Some(ExtractTarget::EntryPath(p)) = rs.extract_target(1) else {
                panic!("blob row extractable");
            };
            assert_eq!(p, "blobs/x:00000001");
        }
    }

    #[test]
    fn main_rows_mark_databases_unextractable() {
        for source in catalogs() {
            let source = source.clone();
            let env = LmdbEnv::open(&source).unwrap();
            let mut rs = LmdbRowSet::open(&source, env.meta.main, None).unwrap();
            rs.ensure_row(1).unwrap();
            let dbs: Vec<usize> = (1..=6)
                .filter(|&i| cell(&rs, i, 1).starts_with("<database"))
                .collect();
            assert_eq!(dbs.len(), 4);
            assert!(rs.extract_target(dbs[0]).is_none());
            let plain = (1..=6).find(|i| !dbs.contains(i)).unwrap();
            assert!(rs.extract_target(plain).is_some());
        }
    }

    #[test]
    fn integer_named_db_keys_render_as_numbers() {
        for source in catalogs() {
            let mut rs = named(&source, b"counters");
            rs.ensure_row(1).unwrap();
            let keys: Vec<String> = (1..=6).map(|i| cell(&rs, i, 0)).collect();
            assert_eq!(keys, ["1", "2", "255", "256", "65536", "4000000000"]);
        }
    }

    #[test]
    fn integer_keys_render_as_numbers() {
        let source = event_log();
        let env = LmdbEnv::open(&source).unwrap();
        let mut rs = LmdbRowSet::open(&source, env.meta.main, None).unwrap();
        rs.ensure_row(1).unwrap();
        assert_eq!(cell(&rs, 1, 0), "1760000000");
        assert_eq!(cell(&rs, 1, 1), "boot");
    }
}
