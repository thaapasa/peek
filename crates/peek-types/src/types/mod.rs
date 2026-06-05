//! Per-file-type modules. Each subdirectory owns the detection
//! contribution, info gathering, info rendering, and view-mode
//! construction for one file type. Cross-cutting layers (input, output,
//! theme, viewer event loop) live elsewhere; type-specific code lives
//! here.

/// `InfoExtras` trait impls wiring each type's stats struct to its
/// `render_section`. Replaces the old central `FileExtras` enum.
mod info_impls;

pub mod archive;
pub mod audio;
pub mod binary;
pub mod cert;
pub mod classfile;
pub mod comic;
pub mod css;
pub mod csv;
pub mod directory;
pub mod disk_image;
pub mod document;
pub mod ebook;
pub mod email;
pub mod eps;
pub mod font;
pub mod html;
pub mod image;
pub mod markdown;
pub mod notebook;
pub mod objfile;
pub mod pdf;
pub mod spreadsheet;
pub mod sql;
pub mod sqlite;
pub mod structured;
pub mod svg;
pub mod text;
pub mod vobject;
