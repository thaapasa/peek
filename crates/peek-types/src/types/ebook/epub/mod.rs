//! EPUB support: ZIP container with HTML chapters + OPF metadata.
//!
//! Three views compose for an `.epub`:
//!
//! - [`read_mode::EpubReader`] (default) — one chapter at a time rendered
//!   via `html2text`, presented through the shared
//!   [`crate::viewer::paged::PagedTextReadMode`] shell; `n` / `p` step
//!   through the spine.
//! - [`crate::viewer::listing::ListingMode`] — TOC view: the raw ZIP
//!   container's file tree (reuses the archive listing pipeline).
//! - [`crate::viewer::modes::InfoMode`] — metadata: title / creator /
//!   language / publisher / spine length, parsed from the OPF.
//!
//! [`package`] owns the OPF / container.xml parsing and the ZIP entry
//! reader; both `read_mode` and the info path go through it.

pub mod info_gather;
pub mod info_render;
pub mod package;
pub mod read_mode;

pub(crate) use read_mode::EpubReader;
