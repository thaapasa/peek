//! SQLite compose path: opens the database, scrapes the catalogue,
//! pushes a [`ListingMode`] showing every schema entity grouped by
//! kind. Contents rows land in step 4; for now each entity contributes
//! a single `.sql` leaf whose extract dumps the `CREATE …` DDL into a
//! temp file (handled by [`super::extract`]).

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::types::sqlite::catalog::{self, Entity, SqliteCatalog};
use crate::types::sqlite::format::SqliteFormat;
use crate::types::sqlite::reader::SqliteReader;
use crate::viewer::ComposeCtx;
use crate::viewer::listing::{Entry, EntryKind, ListingMode};
use crate::viewer::modes::Mode;

/// File suffix used for schema-row inner_paths. Mirrors the SQL viewer
/// the user opens when the row is Enter'd — keeps the listing's leaf
/// names self-describing (`books.sql` reads as "books' DDL").
pub(crate) const SCHEMA_SUFFIX: &str = ".sql";

/// Top-level kind directory names used in inner_paths. Must match the
/// arms in [`super::extract::extract`].
pub(crate) const KIND_TABLES: &str = "tables";
pub(crate) const KIND_VIEWS: &str = "views";
pub(crate) const KIND_INDEXES: &str = "indexes";
pub(crate) const KIND_TRIGGERS: &str = "triggers";

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    _args: &Args,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: SqliteFormat,
) -> Result<()> {
    let (entries, warnings) = match build_entries(source) {
        Ok(es) => (es, Vec::new()),
        Err(e) => (
            Vec::new(),
            vec![format!("Failed to read SQLite catalogue: {e:#}")],
        ),
    };
    modes.push(Box::new(ListingMode::new(
        fmt.label(),
        "Schema",
        entries,
        warnings,
    )));
    Ok(())
}

fn build_entries(source: &InputSource) -> Result<Vec<Entry>> {
    let reader = SqliteReader::open(source)?;
    let catalog = catalog::load(&reader.conn)?;
    Ok(catalog_to_entries(&catalog))
}

fn catalog_to_entries(catalog: &SqliteCatalog) -> Vec<Entry> {
    let mut out = Vec::with_capacity(4);
    push_group(&mut out, KIND_TABLES, &catalog.tables);
    push_group(&mut out, KIND_VIEWS, &catalog.views);
    push_group(&mut out, KIND_INDEXES, &catalog.indexes);
    push_group(&mut out, KIND_TRIGGERS, &catalog.triggers);
    out
}

fn push_group(out: &mut Vec<Entry>, name: &str, entities: &[Entity]) {
    if entities.is_empty() {
        return;
    }
    let children: Vec<Entry> = entities.iter().map(schema_entry).collect();
    out.push(Entry {
        name: name.to_string(),
        size: 0,
        mtime: None,
        mode: None,
        kind: EntryKind::Dir { children },
    });
}

fn schema_entry(entity: &Entity) -> Entry {
    // `size` doubles as the listing's size column — use the DDL byte
    // length when SQLite recorded one, otherwise 0. Cheap, informative
    // for users skimming the listing.
    let size = entity.sql.as_deref().map(|s| s.len() as u64).unwrap_or(0);
    Entry {
        name: format!("{}{}", entity.name, SCHEMA_SUFFIX),
        size,
        mtime: None,
        mode: None,
        kind: EntryKind::File,
    }
}
