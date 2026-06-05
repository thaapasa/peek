//! Per-file-type detection contributions: each module owns its format
//! enum plus the pure extension / MIME / content-sniff helpers the
//! orchestrator ([`crate::detect`]) calls. No reader / viewer code lives
//! here — that stays in the `peek` binary's `types/` tree, which imports
//! these format enums back through `peek_detect`.

pub mod archive;
pub mod audio;
pub mod cert;
pub mod comic;
pub mod csv;
pub mod disk_image;
pub mod document;
pub mod ebook;
pub mod email;
pub mod eps;
pub mod font;
pub mod objfile;
pub mod pdf;
pub mod spreadsheet;
pub mod sqlite;
pub mod structured;
pub mod vobject;
