//! SQLite database support.
//!
//! Read-only introspection via `rusqlite` (bundled libsqlite3). The
//! info-side ships first: file-level pragmas (page size, encoding,
//! integrity), schema counts, and a "biggest tables" snapshot — all
//! gathered in `info_gather` and rendered in `info_render`. A
//! listing + table view follow in later patches; until then the
//! body falls through to the universal Hex view.

pub mod catalog;
pub mod compose;
pub mod detect;
pub mod extract;
pub mod format;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod reader;
pub mod row_set;
pub mod sql;
pub mod table_mode;
