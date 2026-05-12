//! ODT (OpenDocument Text) support: ZIP container with `content.xml`
//! body, `meta.xml` Dublin Core metadata, and optional `styles.xml`.
//!
//! Three views compose for an `.odt`:
//!
//! - [`crate::types::document::DocReadMode`] (default) — styled text,
//!   parsed once at open time into a shared [`super::ast::Doc`] AST and
//!   re-laid-out per width on each render.
//! - [`crate::types::listing::ListingMode`] — TOC view: the raw ZIP
//!   container's file tree (reuses the archive listing pipeline).
//! - [`crate::viewer::modes::InfoMode`] — metadata: title / creator /
//!   subject / paragraph and word counts, parsed from `meta.xml`.
//!
//! [`package`] owns the XML walk and AST conversion; `read_mode` and
//! `info_gather` both go through it.

pub mod info_gather;
pub mod package;
