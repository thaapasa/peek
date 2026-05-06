//! Per-file-type modules. Each subdirectory owns the detection
//! contribution, info gathering, info rendering, and view-mode
//! construction for one file type. Cross-cutting layers (input, output,
//! theme, viewer event loop) live elsewhere; type-specific code lives
//! here.

pub mod archive;
pub mod binary;
pub mod disk_image;
pub mod image;
pub mod markdown;
pub mod sql;
pub mod structured;
pub mod svg;
pub mod text;
