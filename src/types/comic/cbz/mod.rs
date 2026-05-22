//! CBZ support: ZIP container with one image per page.
//!
//! Two views compose for a `.cbz`:
//!
//! - [`page_renderer::CbzPageRenderer`] (default) — one page at a time
//!   rendered as ASCII art via the image pipeline, wrapped in the
//!   generic [`crate::viewer::paged::PagedImageMode`]; `n` / `p` step
//!   through pages.
//! - [`crate::viewer::listing::ListingMode`] — TOC view: the raw ZIP
//!   container's file tree (reuses the archive listing pipeline).
//!
//! [`package`] owns the ZIP entry walk that picks image pages out of
//! the container in name order; both `page_renderer` and the info path
//! go through it.

pub mod info_gather;
pub mod info_render;
pub mod package;
pub mod page_renderer;

pub(crate) use page_renderer::CbzPageRenderer;
