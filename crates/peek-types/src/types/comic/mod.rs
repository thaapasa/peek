//! Comic-archive formats. Per-format submodule (currently `cbz`)
//! owns container parsing, paged-image read mode, and info gather /
//! render for one container shape; [`info`] holds the shared stats
//! struct populated per-format.

pub mod cbz;
pub mod compose;
pub mod info;

pub(crate) use cbz::CbzPageRenderer;
pub use info::ComicStats;
