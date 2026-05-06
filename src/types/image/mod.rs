//! Image support: raster + SVG-rasterized rendering, animated GIF/WebP
//! playback, animated SVG (CSS keyframes) playback, and per-image
//! metadata gathering.
//!
//! Submodules:
//! - `pipeline` — rasterization → ASCII-art rendering core (used by all
//!   image-displaying modes including SVG variants in `types::svg`).
//!   Also exposes `svg_anim` (CSS-keyframe parser/timeline) for the SVG
//!   animation mode.
//! - `mode` / `animation_mode` — interactive view modes for static images
//!   and raster animations (GIF/WebP).
//! - `info_gather` / `info_render` — image metadata extraction (EXIF, XMP,
//!   HDR, ICC, animation header walk) and the Image info section.
//! - `exif` / `xmp` / `animation_stats` — gather subhelpers used from
//!   `info_gather`.

pub mod animation_mode;
pub mod animation_stats;
pub mod exif;
pub mod info_gather;
pub mod info_render;
pub mod mode;
pub mod pipeline;
pub mod xmp;

pub(crate) use animation_mode::AnimationMode;
pub(crate) use mode::{ImageKind, ImageRenderMode};
pub use pipeline::{Background, FitMode, ImageConfig, ImageMode};
