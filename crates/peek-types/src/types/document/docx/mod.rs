//! DOCX support: ZIP container with `word/document.xml` body + `docProps`
//! metadata. Three views compose for a `.docx`:
//!
//! - [`crate::types::document::DocReadMode`] (default) — styled text,
//!   walked once at open time into a shared [`crate::types::document::ast::Doc`]
//!   AST and re-laid-out per width on each render.
//! - [`crate::viewer::listing::ListingMode`] — TOC view: the raw ZIP
//!   container's file tree (reuses the archive listing pipeline).
//! - [`crate::viewer::modes::InfoMode`] — metadata: title / creator /
//!   subject / paragraph and word counts, parsed from `docProps/core.xml`.
//!
//! [`package`] owns the XML walk + AST conversion (shared AST in
//! `super::ast`); `info_gather` re-runs `package::open` for the info
//! section.

pub mod info_gather;
pub mod package;
