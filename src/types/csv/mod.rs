//! CSV / TSV support: aligned table view, streaming record reader,
//! per-column type inference.

pub mod compose;
pub mod detect;
pub mod format;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod parse;
pub mod table_mode;

pub use info::CsvStats;
