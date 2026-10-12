//! LMDB compose: with named sub-databases, land on a listing of them
//! (`@main` first when MAIN holds plain keys too) where Enter opens a DB's
//! row table; with none, land straight on MAIN's table. Rows carry a
//! cursor — Enter / `e` open the selected value through
//! [`super::extract`]. A file that won't open pushes nothing, so Info
//! (appended below) lands and shows why.

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use bytes::Bytes;
use peek_detect::Detected;
use peek_io::InputSource;

use super::cells::{self, MAIN_SEGMENT};
use super::env::LmdbEnv;
use super::format::DbRecord;
use super::row_set::LmdbRowSet;
use crate::viewer::listing::{Entry, EntryKind, ListingMode};
use crate::viewer::modes::{DescendFrame, ExtractTarget, InfoMode, Mode};
use crate::viewer::table::rows_mode::RowsTableMode;
use crate::viewer::{ComposeCtx, ComposeOpts};

const FORMAT_LABEL: &str = "LMDB";
const TABLE_LABEL: &str = "Rows";

/// One database a listing row opens: MAIN key (`None` = MAIN) + record.
#[derive(Clone)]
struct DbTarget {
    name: Option<Bytes>,
    db: DbRecord,
}

pub fn compose(
    source: &InputSource,
    detected: &Detected,
    _args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    if let Ok(mode) = landing_mode(source, detected) {
        modes.push(mode);
    }
    // The universal tail also appends Info; this push fixes the ordering.
    modes.push(Box::new(InfoMode::new()));
    Ok(())
}

fn landing_mode(source: &InputSource, detected: &Detected) -> Result<Box<dyn Mode>> {
    let env = LmdbEnv::open(source)?;
    let main = env.meta.main;
    let named = env.named_dbs()?;
    if named.dbs.is_empty() {
        return Ok(Box::new(table_mode(source, main, None)?));
    }

    let mut targets: HashMap<String, DbTarget> = HashMap::new();
    let mut entries = Vec::with_capacity(named.dbs.len() + 1);
    // MAIN's entries count its named-DB records too; list MAIN only when
    // it also holds plain keys.
    if main.entries > named.dbs.len() as u64 {
        entries.push(db_entry(MAIN_SEGMENT.to_string(), &main, env.psize));
        targets.insert(
            MAIN_SEGMENT.to_string(),
            DbTarget {
                name: None,
                db: main,
            },
        );
    }
    for d in named.dbs {
        let seg = cells::encode_segment(&d.name);
        entries.push(db_entry(seg.clone(), &d.db, env.psize));
        targets.insert(
            seg,
            DbTarget {
                name: Some(d.name),
                db: d.db,
            },
        );
    }
    let warnings = if named.truncated {
        vec!["MAIN scan stopped early — more databases may exist".to_string()]
    } else {
        Vec::new()
    };

    let source = source.clone();
    let detected = detected.clone();
    let listing = ListingMode::new(FORMAT_LABEL, "Databases", entries, warnings)
        .with_descend_handler(move |target| {
            let ExtractTarget::EntryPath(key) = target else {
                return None;
            };
            let t = targets.get(key)?;
            Some(db_frame(&source, &detected, t, key))
        });
    Ok(Box::new(listing))
}

/// Listing row. The size column is a byte column (human-size toggle,
/// byte gradient), so it shows the DB's pages on disk; the entry count
/// lives in Info.
fn db_entry(name: String, db: &DbRecord, psize: u64) -> Entry {
    Entry {
        name,
        size: db.total_pages().saturating_mul(psize),
        mtime: None,
        mode: None,
        kind: EntryKind::File,
    }
}

fn table_mode(source: &InputSource, db: DbRecord, name: Option<&[u8]>) -> Result<RowsTableMode> {
    let rows = LmdbRowSet::open(source, db, name)?;
    Ok(
        RowsTableMode::new(Box::new(rows), LmdbRowSet::alignments(), true, TABLE_LABEL)
            .with_row_cursor(),
    )
}

fn db_frame(
    source: &InputSource,
    detected: &Detected,
    target: &DbTarget,
    label: &str,
) -> Result<DescendFrame> {
    let table = table_mode(source, target.db, target.name.as_deref())
        .map_err(|e| anyhow!("opening {label}: {e:#}"))?;
    let mut modes: Vec<Box<dyn Mode>> = vec![Box::new(table)];
    // No Hex: the frame reuses the whole-file source, so a hex dump would
    // show the environment, not this database.
    crate::viewer::append_universal_modes(&mut modes, None)?;
    Ok(DescendFrame {
        source: source.clone(),
        detected: detected.clone(),
        modes,
        breadcrumb_label: Some(label.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::lmdb::testing::catalogs;

    #[test]
    fn listing_size_is_bytes_on_disk() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source).unwrap();
            let users = env
                .named_dbs()
                .unwrap()
                .dbs
                .into_iter()
                .find(|d| &d.name[..] == b"users")
                .unwrap()
                .db;
            let entry = db_entry("users".to_string(), &users, env.psize);
            assert_eq!(entry.size, users.total_pages() * env.psize);
            assert_ne!(entry.size, users.entries);
        }
    }
}
