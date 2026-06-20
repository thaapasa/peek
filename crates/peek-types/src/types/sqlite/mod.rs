//! SQLite database support.
//!
//! Read-only introspection via `rusqlite` (bundled libsqlite3). The
//! info side surfaces file-level pragmas (page size, encoding,
//! integrity), schema counts, and a "biggest tables" snapshot —
//! gathered in `info_gather`, rendered in `info_render`. The body is
//! a schema listing (`compose`) that descends into a streaming
//! per-table row view (`table_mode`).

pub mod catalog;
pub mod compose;
pub mod extract;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod reader;
pub mod row_set;
pub mod sql;
pub mod table_mode;

/// Format enum, re-exported from `peek_detect` at the module root so
/// reader code keeps a local `crate::types::sqlite::SqliteFormat` path.
pub use peek_detect::types::sqlite::SqliteFormat;
