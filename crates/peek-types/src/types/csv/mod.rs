//! CSV / TSV support: aligned table view, streaming record reader,
//! per-column type inference.

pub mod compose;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod parse;

pub use info::CsvStats;

/// Format enum, re-exported from `peek_detect` at the module root so
/// reader code keeps a local `crate::types::csv::CsvFormat` path.
pub use peek_detect::types::csv::CsvFormat;
