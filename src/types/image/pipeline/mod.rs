//! Image rendering pipeline for raster + SVG sources.
//!
//! - [`render`], [`animate`], [`clustering`], [`glyph_atlas`] make up the
//!   shared rasterization → ASCII-art pipeline used by the interactive
//!   `ImageRenderMode` and `AnimationMode`.
//! - [`ImageConfig`] / [`ImageMode`] / [`Background`] configure that
//!   pipeline; they flow in from CLI args via `Registry`.

pub(crate) mod animate;
mod clustering;
mod contour;
pub(crate) mod glyph_atlas;
pub mod render;
pub(crate) mod svg;
pub(crate) mod svg_anim;

#[cfg(test)]
mod tests;

// The render-config vocabulary (ImageMode / Background / FitMode /
// ImageConfig) moved to the foundation (`crate::viewer::image_render`) so
// the shared paged mode can read it without depending on this engine.
// Re-exported here so the engine's `super::` / `pipeline::` paths stay put.
pub use crate::viewer::image_render::{Background, FitMode, ImageConfig, ImageMode};
