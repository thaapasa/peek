//! Word-style documents: DOCX (Office Open XML), ODT (OpenDocument
//! Text), and RTF (Rich Text Format).
//!
//! DOCX and ODT share an AST + renderer + read mode ([`ast`], [`render`],
//! [`read_mode`]). Each per-format submodule owns only its parser +
//! per-format info gather; both feed the same shared pipeline so the
//! viewer stays format-agnostic from the read view down.
//!
//! RTF stays separate: its on-the-wire structure is a flat stream of
//! painter-tagged text, not a paragraph/run tree, so forcing it into
//! the shared AST would mean re-flowing every block.
//!
//! DOCX + ODT are ZIP containers — the read view shows styled text, the
//! TOC view exposes the inner ZIP, and `--extract` reuses the archive
//! ZIP path. RTF is single-file: read view + (when present) embed
//! listing.

pub mod ast;
pub mod compose;
pub mod detect;
pub mod docx;
pub mod format;
pub mod info;
pub mod info_render;
pub mod odt;
pub mod read_mode;
pub mod render;
pub mod rtf;

pub use info::{DocumentMetadata, DocumentStats};
pub(crate) use read_mode::DocReadMode;
