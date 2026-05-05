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
pub(crate) mod svg_anim;

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

    pub fn prev(self) -> Self {
        match self {
            Self::Auto => Self::Checkerboard,
            Self::Black => Self::Auto,
            Self::White => Self::Black,
            Self::Checkerboard => Self::White,
        }
    }
}

/// Which axis constrains the rendered image grid relative to the terminal
/// viewport. The image is never rotated; only the fitting/scrolling
/// behavior changes.
///
/// - `Contain`: scale to fit both terminal width and height (current
///   default). Neither axis overflows; the viewer never scrolls the
///   image.
/// - `FitWidth`: scale to fill terminal width. Height grows to preserve
///   aspect ratio and may exceed the terminal — the viewer scrolls the
///   image vertically.
/// - `FitHeight`: scale to fill terminal height. Width grows similarly
///   and may exceed the terminal — the viewer scrolls the image
///   horizontally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Contain,
    FitWidth,
    FitHeight,
}

impl FitMode {
    pub fn next(self) -> Self {
        match self {
            Self::Contain => Self::FitWidth,
            Self::FitWidth => Self::FitHeight,
            Self::FitHeight => Self::Contain,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Contain => "Contain",
            Self::FitWidth => "FitWidth",
            Self::FitHeight => "FitHeight",
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
    /// Which terminal axis constrains the rendered image grid. Toggled
    /// interactively with `f`; CLI default is `Contain`. Ignored by the
    /// pipe / `--print` path, which always uses `Contain` (rows are
    /// unbounded there, so `FitHeight` is meaningless and `FitWidth`
    /// reduces to `Contain` anyway).
    pub fit: FitMode,
}
