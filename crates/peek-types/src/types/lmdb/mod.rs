//! LMDB (Lightning Memory-Mapped Database) support — `data.mdb` or a
//! no-subdir single-file environment.
//!
//! Read-only, pure Rust page walker over a `ByteSource`: no liblmdb, no
//! mmap, and the lock file is never touched. `compose` lands on a database
//! listing (or MAIN's rows when there are no named DBs); rows stream
//! through the shared `RowsTableMode` with a cursor, and Enter / `e` open
//! one value via `extract`.

pub mod cells;
pub mod compose;
pub mod env;
pub mod extract;
pub mod format;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod row_set;
pub mod tree;

#[cfg(test)]
pub(crate) mod testing {
    use peek_io::InputSource;

    /// No-subdir file, 16 KiB pages, LMDB 1.0: MAIN + `users` / `tags`
    /// (dupsort) / `blobs` (binary, one overflow value).
    pub const CATALOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-data/catalog.lmdb");
    /// The same data written by LMDB 0.9.
    pub const CATALOG_09: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-data/catalog-0.9.lmdb"
    );
    /// Directory-layout 1.0 data file: one INTEGERKEY MAIN, 24 entries.
    pub const EVENT_LOG: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-data/event-log.lmdb/data.mdb"
    );

    pub fn catalog() -> InputSource {
        InputSource::File(CATALOG.into())
    }

    /// Both catalog fixtures: 1.0 and 0.9 layouts.
    pub fn catalogs() -> [InputSource; 2] {
        [catalog(), InputSource::File(CATALOG_09.into())]
    }

    pub fn event_log() -> InputSource {
        InputSource::File(EVENT_LOG.into())
    }
}
