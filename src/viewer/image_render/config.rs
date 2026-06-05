//! Image-render configuration + terminal-dimension vocabulary.
//!
//! Pure value types shared by the foundation-level paged image mode and
//! the `types/image` rasterization engine — the config the engine reads
//! and the terminal size it's fed. No rendering logic lives here.

use crate::theme::StyleMode;

use super::image_mode::ImageMode;

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
    pub style_mode: StyleMode,
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

/// Terminal dimensions in characters. The image renderer is fed sizes
/// from `RenderCtx` rather than querying the terminal itself, so the
/// same code path serves both interactive (live terminal size) and
/// pipe (`$COLUMNS or 80`, unbounded rows) rendering.
#[derive(Debug, Clone, Copy)]
pub struct TermSize {
    pub cols: u32,
    pub rows: u32,
    /// Terminal cell aspect (height ÷ width). Conventional fonts hit
    /// ~2.0; tighter programming fonts run ~1.6–2.4. Auto-detected
    /// from the running terminal at startup, with a `--cell-aspect`
    /// CLI override.
    pub cell_h_over_w: f64,
}
