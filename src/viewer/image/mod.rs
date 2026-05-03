//! Image rendering pipeline for raster + SVG sources.
//!
//! - [`render`], [`animate`], [`clustering`], [`glyph_atlas`] make up the
//!   shared rasterization → ASCII-art pipeline used by the interactive
//!   `ImageRenderMode` and `AnimationMode`.
//! - [`ImageConfig`] / [`ImageMode`] / [`Background`] configure that
//!   pipeline; they flow in from CLI args via `Registry`.

use crate::theme::ColorMode;

pub(crate) mod animate;
mod clustering;
mod contour;
mod glyph_atlas;
mod mode;
pub mod render;
mod svg;

#[cfg(test)]
mod tests;

pub use mode::ImageMode;

/// Background mode for transparency compositing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    /// Auto-detect: dark content → white bg, light content → black bg
    Auto,
    /// Solid black
    Black,
    /// Solid white
    White,
    /// Checkerboard pattern
    Checkerboard,
}

impl Background {
    pub fn from_str(s: &str) -> Self {
        match s {
            "black" => Self::Black,
            "white" => Self::White,
            "checkerboard" | "checker" => Self::Checkerboard,
            _ => Self::Auto,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Black,
            Self::Black => Self::White,
            Self::White => Self::Checkerboard,
            Self::Checkerboard => Self::Auto,
        }
    }
}

/// Shared configuration for image rendering (mode, size, background, margin).
#[derive(Debug, Clone, Copy)]
pub struct ImageConfig {
    pub mode: ImageMode,
    pub width: u32,
    pub background: Background,
    pub margin: u32,
    pub color_mode: ColorMode,
    /// Target fraction of pixels marked as edges in `ImageMode::Contour`.
    /// Range 0.0..1.0. Higher = denser line-art. Stable across animation
    /// frames because it's a percentile of the gradient histogram.
    pub edge_density: f32,
}
