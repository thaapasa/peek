//! Word-style documents: DOCX (Office Open XML) and RTF (Rich Text
//! Format). Per-format submodules own detection, info gather, info
//! render, and view-mode construction; [`info`] holds the shared
//! metadata struct they both populate.
//!
//! DOCX is a ZIP container — the read-view shows styled text, the
//! TOC view exposes the inner ZIP, and `--extract` reuses the archive
//! ZIP path. RTF is single-file: read-view only.

pub mod docx;
pub mod info;
pub mod info_render;
pub mod rtf;

pub use info::{DocumentMetadata, DocumentStats};
