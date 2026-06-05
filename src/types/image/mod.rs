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

pub(crate) mod anim_frame;
pub mod animation_mode;
pub mod animation_stats;
pub mod compose;
pub mod exif;
pub mod extract;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod mode;
pub(crate) mod paged_render;
pub mod pipeline;
pub(crate) mod view;
pub mod xmp;
// The zoom / scroll / zoom_pan geometry + the render-config vocab moved to
// the foundation (`crate::viewer::image_render`) so the shared paged mode
// can use them without depending on this engine. Re-exported at the old
// module paths so this crate's `super::zoom::*` / `image::scroll::*` uses
// are unchanged (reader → foundation, the allowed direction).
pub(crate) use crate::viewer::image_render::{scroll, zoom, zoom_pan};

pub(crate) use animation_mode::AnimationMode;
pub(crate) use mode::{ImageKind, ImageRenderMode};
