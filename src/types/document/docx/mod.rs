//! DOCX support: ZIP container with `word/document.xml` body + `docProps`
//! metadata. Three views compose for a `.docx`:
//!
//! - [`read_mode::DocxReadMode`] (default) — styled text, walked once at
//!   open time into an owned [`package::Doc`] AST and re-laid-out per
//!   width on each render.
//! - [`crate::types::listing::ListingMode`] — TOC view: the raw ZIP
//!   container's file tree (reuses the archive listing pipeline).
//! - [`crate::viewer::modes::InfoMode`] — metadata: title / creator /
//!   subject / paragraph and word counts, parsed from `docProps/core.xml`.
//!
//! [`package`] owns the docx-rust parse and the AST conversion; `read_mode`
//! and `info_gather` both go through it.

pub mod info_gather;
pub mod package;
pub mod read_mode;
pub mod render;

pub(crate) use read_mode::DocxReadMode;
