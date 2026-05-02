//! Image and SVG viewers plus their shared rendering pipeline.
//!
//! - [`ImageViewer`] / [`SvgViewer`] are the print-mode [`Viewer`]
//!   implementations (one per format).
//! - [`render`], [`animate`], [`clustering`], [`glyph_atlas`] make up the
//!   shared rasterization → ASCII-art pipeline.
//! - [`ImageConfig`] / [`ImageMode`] / [`Background`] configure that
//!   pipeline; they're shared with the interactive image modes too.

use crate::theme::ColorMode;

use super::Viewer;

pub(crate) mod animate;
mod clustering;
mod glyph_atlas;
mod mode;
pub mod render;
mod svg;
mod viewer;

pub use mode::ImageMode;
pub use svg::SvgViewer;
pub use viewer::ImageViewer;

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
}
