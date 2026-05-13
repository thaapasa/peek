//! E-book formats. Per-format submodules (currently just `epub`) own
//! detection, info gather, info render, and view-mode construction
//! for one container shape; [`info`] holds the shared metadata struct
//! they all populate.

pub mod compose;
pub mod detect;
pub mod epub;
pub mod format;
pub mod info;

pub use info::{EbookStats, Metadata};
