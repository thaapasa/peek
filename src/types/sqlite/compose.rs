//! SQLite compose path: opens the database, scrapes the catalogue,
//! pushes a [`ListingMode`] showing every schema entity grouped by
//! kind. Contents rows land in step 4; for now each entity contributes
//! a single `.sql` leaf whose extract dumps the `CREATE …` DDL into a
//! temp file (handled by [`super::extract`]).

use anyhow::{Result, anyhow};

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::types::sqlite::catalog::{self, Entity, SqliteCatalog};
use crate::types::sqlite::format::SqliteFormat;
use crate::types::sqlite::reader::SqliteReader;
use crate::types::sqlite::table_mode::build as build_table_mode;
use crate::viewer::ComposeCtx;
use crate::viewer::listing::{Entry, EntryKind, ListingMode};
use crate::viewer::modes::{AboutMode, DescendFrame, ExtractTarget, InfoMode, Mode};

/// File suffix used for schema-row inner_paths. Mirrors the SQL viewer
/// the user opens when the row is Enter'd — keeps the listing's leaf
/// names self-describing (`books.sql` reads as "books' DDL").
pub(crate) const SCHEMA_SUFFIX: &str = ".sql";

/// File suffix used for contents-row inner_paths. Drilling in via Enter
/// pushes a streaming row viewer; extracting via `e` dumps the rows
/// to a CSV file on disk (see [`super::extract`]).
pub(crate) const CONTENTS_SUFFIX: &str = ".csv";

/// Top-level kind directory names used in inner_paths. Must match the
/// arms in [`super::extract::extract`].
pub(crate) const KIND_TABLES: &str = "tables";
pub(crate) const KIND_VIEWS: &str = "views";
pub(crate) const KIND_INDEXES: &str = "indexes";
pub(crate) const KIND_TRIGGERS: &str = "triggers";

pub fn compose(
    source: &InputSource,
    detected: &Detected,
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
    // Descend on a `<entity>.csv` row opens a streaming table viewer
    // over the *current* database instead of extracting to a temp file.
    // The closure reuses these clones so each pushed frame owns its own
    // read-only connection and reuses the original detection (so InfoMode
    // still renders the SQLite metadata over the same DB).
    let descend_source = source.clone();
    let descend_detected = detected.clone();
    let listing = ListingMode::new(fmt.label(), "Schema", entries, warnings).with_descend_handler(
        move |target| {
            let ExtractTarget::EntryPath(key) = target else {
                return None;
            };
            let contents = parse_contents_key(key)?;
            Some(build_contents_frame(
                &descend_source,
                &descend_detected,
                &contents,
            ))
        },
    );
    modes.push(Box::new(listing));
    Ok(())
}

struct ContentsTarget {
    entity: String,
}

/// Pick a contents-row `<kind>/<entity>.csv` inner_path apart into its
/// entity. `None` for any other selection — schema rows (`.sql`), group
/// directories, non-row-bearing kinds.
fn parse_contents_key(key: &str) -> Option<ContentsTarget> {
    let (kind_dir, rest) = key.split_once('/')?;
    let name = rest.strip_suffix(CONTENTS_SUFFIX)?;
    if name.is_empty() || name.contains('/') {
        return None;
    }
    // Only tables and views are row-bearing. Indexes / triggers never
    // get a `.csv` row written by compose, but a defensive check here
    // keeps a future bug there from opening a SqliteRowSet on something
    // that has no rows.
    match kind_dir {
        KIND_TABLES | KIND_VIEWS => Some(ContentsTarget {
            entity: name.to_string(),
        }),
        _ => None,
    }
}

fn build_contents_frame(
    source: &InputSource,
    detected: &Detected,
    target: &ContentsTarget,
) -> Result<DescendFrame> {
    let table = build_table_mode(source, &target.entity)
        .map_err(|e| anyhow!("opening {}: {e:#}", target.entity))?;
    let modes: Vec<Box<dyn Mode>> = vec![
        Box::new(table),
        Box::new(InfoMode::new()),
        Box::new(AboutMode::new()),
    ];
    Ok(DescendFrame {
        source: source.clone(),
        detected: detected.clone(),
        modes,
        // Reuses the db source, so label the crumb with the table name
        // instead of repeating the db file.
        breadcrumb_label: Some(target.entity.clone()),
    })
}

fn build_entries(source: &InputSource) -> Result<Vec<Entry>> {
    let reader = SqliteReader::open(source)?;
    let catalog = catalog::load(&reader.conn)?;
    Ok(catalog_to_entries(&catalog))
}

fn catalog_to_entries(catalog: &SqliteCatalog) -> Vec<Entry> {
    let mut out = Vec::with_capacity(4);
    push_group(
        &mut out,
        KIND_TABLES,
        &catalog.tables,
        /*row_bearing=*/ true,
    );
    push_group(
        &mut out,
        KIND_VIEWS,
        &catalog.views,
        /*row_bearing=*/ true,
    );
    push_group(
        &mut out,
        KIND_INDEXES,
        &catalog.indexes,
        /*row_bearing=*/ false,
    );
    push_group(
        &mut out,
        KIND_TRIGGERS,
        &catalog.triggers,
        /*row_bearing=*/ false,
    );
    out
}

fn push_group(out: &mut Vec<Entry>, name: &str, entities: &[Entity], row_bearing: bool) {
    if entities.is_empty() {
        return;
    }
    let mut children: Vec<Entry> = Vec::with_capacity(entities.len() * 2);
    for entity in entities {
        children.push(schema_entry(entity));
        if row_bearing {
            children.push(contents_entry(entity));
        }
    }
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

fn contents_entry(entity: &Entity) -> Entry {
    // `size` mirrors the row count for contents rows so users scanning
    // the listing can compare table populations at a glance. Not the
    // byte size, deliberately — bytes are uninteresting for a logical
    // row view and we already surface the database's page count in
    // InfoMode.
    Entry {
        name: format!("{}{}", entity.name, CONTENTS_SUFFIX),
        size: entity.row_count,
        mtime: None,
        mode: None,
        kind: EntryKind::File,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_contents_key_accepts_tables_and_views() {
        let t = parse_contents_key("tables/books.csv").expect("tables row");
        assert_eq!(t.entity, "books");
        let v = parse_contents_key("views/popular_authors.csv").expect("views row");
        assert_eq!(v.entity, "popular_authors");
    }

    #[test]
    fn parse_contents_key_rejects_non_row_bearing() {
        assert!(parse_contents_key("indexes/idx_x.csv").is_none());
        assert!(parse_contents_key("triggers/trg.csv").is_none());
    }

    #[test]
    fn parse_contents_key_rejects_schema_rows() {
        assert!(parse_contents_key("tables/books.sql").is_none());
    }

    #[test]
    fn parse_contents_key_rejects_bad_shapes() {
        assert!(parse_contents_key("books.csv").is_none(), "missing kind");
        assert!(parse_contents_key("tables/.csv").is_none(), "empty name");
        assert!(parse_contents_key("tables/a/b.csv").is_none(), "nested");
    }
}
