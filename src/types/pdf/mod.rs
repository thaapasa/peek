//! PDF support: paged-image render, text extraction, and embedded-files
//! listing. Built on Pdfium (Google's PDF library, dynamically loaded
//! from a `libpdfium.*` shipped alongside the peek binary).
//!
//! The three modes mirror existing patterns:
//!   * [`PdfPageMode`] — paged rasterizer, one page at a time
//!     ([`crate::types::comic::cbz::CbzReadMode`] analog)
//!   * [`PdfTextMode`] — width-cached text render
//!     ([`crate::types::document::DocReadMode`] analog)
//!   * [`crate::types::listing::ListingMode`] of `/EmbeddedFiles`
//!     attachments — extract path lives in [`extract`]

pub mod extract;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod package;
pub mod page_mode;
pub mod text_mode;

pub use info::PdfStats;
pub(crate) use page_mode::PdfPageMode;
pub(crate) use text_mode::PdfTextMode;
