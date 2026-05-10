//! CBZ support: ZIP container with one image per page.
//!
//! Two views compose for a `.cbz`:
//!
//! - [`read_mode::CbzReadMode`] (default) — one page at a time
//!   rendered as ASCII art via the image pipeline; `n` / `N` step
//!   through pages.
//! - [`crate::types::listing::ListingMode`] — TOC view: the raw ZIP
//!   container's file tree (reuses the archive listing pipeline).
//!
//! [`package`] owns the ZIP entry walk that picks image pages out of
//! the container in name order; both `read_mode` and the info path
//! go through it.

pub mod info_gather;
pub mod info_render;
pub mod package;
pub mod read_mode;

pub(crate) use read_mode::CbzReadMode;
