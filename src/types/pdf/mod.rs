//! PDF support: paged-image render, text extraction, and embedded-files
//! listing. Built on Pdfium (Google's PDF library, dynamically loaded
//! from a `libpdfium.*` shipped alongside the peek binary).
//!
//! The three modes mirror existing patterns:
//!   * [`PdfPageRenderer`] — paged rasterizer, one page at a time,
//!     wrapped in the generic [`crate::viewer::paged::PagedImageMode`]
//!     ([`crate::types::comic::cbz::CbzPageRenderer`] analog)
//!   * [`PdfTextRenderer`] — width-cached text render, wrapped in the
//!     generic [`crate::viewer::modes::RenderedTextMode`]
//!   * [`crate::viewer::listing::ListingMode`] of `/EmbeddedFiles`
//!     attachments — extract path lives in [`extract`]

pub mod compose;
pub mod extract;
pub mod format;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod package;
pub mod page_renderer;
pub mod text_renderer;

pub use info::PdfStats;
pub(crate) use page_renderer::PdfPageRenderer;
pub(crate) use text_renderer::PdfTextRenderer;
