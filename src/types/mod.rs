//! Per-file-type modules. Each subdirectory owns the detection
//! contribution, info gathering, info rendering, and view-mode
//! construction for one file type. Cross-cutting layers (input, output,
//! theme, viewer event loop) live elsewhere; type-specific code lives
//! here.

pub mod archive;
pub mod binary;
pub mod comic;
pub mod disk_image;
pub mod document;
pub mod ebook;
pub mod html;
pub mod image;
pub mod listing;
pub mod markdown;
pub mod sql;
pub mod structured;
pub mod svg;
pub mod text;
