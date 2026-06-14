//! Presentations: PPTX / PPTM / PPSX (Office Open XML), ODP
//! (OpenDocument Presentation), and Keynote (`.key`).
//!
//! The missing leg of the office trio — `document` ships Word, the
//! `spreadsheet` type ships Excel; this ships slides. PPTX and ODP parse
//! into a per-slide [`crate::types::document::ast::Doc`] and reuse the
//! shared document prose renderer, presented one slide at a time through
//! the shared [`crate::viewer::paged::PagedTextReadMode`] shell (a
//! [`read_mode::PresentationReader`] — same shell the EPUB `n` / `p`
//! chapter flow uses) plus the raw ZIP-entry TOC listing.
//!
//! Keynote is the exception. Modern `.key` stores slide text as
//! undocumented snappy-protobuf (IWA), which there's no usable Rust
//! library for, so slide-text extraction is out of scope. Instead the
//! embedded `preview.jpg` deck thumbnail renders through the image
//! pipeline ([`keynote::preview`]) alongside the ZIP TOC, and the Info
//! section surfaces what metadata is cheap to read (creating app).
//!
//! Per-format parsing lives in the [`pptx`] / [`odp`] / [`keynote`]
//! submodules; [`info`] holds the shared stats struct they populate and
//! [`deck`] the shared parsed-deck model (slides as `Doc`s).

pub mod compose;
pub mod deck;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod keynote;
pub mod odp;
pub mod pptx;
pub mod read_mode;

pub use deck::Deck;
pub use info::{PresentationMetadata, PresentationStats};

// Re-export the format enum at the module root, mirroring the other type
// modules (the enum + sniff helpers live in `peek-detect`).
pub use crate::input::detect::PresentationFormat;
