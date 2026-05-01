use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::FileType;
use crate::output::Output;
use crate::theme::ColorMode;

use super::Viewer;

pub(crate) mod animate;
mod clustering;
mod glyph_atlas;
pub mod render;
mod svg;

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

/// Image rendering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMode {
    /// All glyphs: ASCII + block elements
    Full,
    /// Unicode block/quadrant elements + curated ASCII subset
    Block,
    /// Unicode block/quadrant elements + line segments (/\|-_) only
    Geo,
    /// Legacy density-ramp renderer (brightness-based, foreground only)
    Ascii,
}

impl ImageMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "block" => Self::Block,
            "geo" => Self::Geo,
            "ascii" => Self::Ascii,
            _ => Self::Full,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Full => Self::Block,
            Self::Block => Self::Geo,
            Self::Geo => Self::Ascii,
            Self::Ascii => Self::Full,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Block => "block",
            Self::Geo => "geo",
            Self::Ascii => "ascii",
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

pub struct ImageViewer {
    config: ImageConfig,
}

impl ImageViewer {
    pub fn new(config: ImageConfig) -> Self {
        Self { config }
    }
}

impl Viewer for ImageViewer {
    fn render(
        &self,
        source: &InputSource,
        _file_type: &FileType,
        output: &mut Output,
    ) -> Result<()> {
        let term = render::TermSize::detect();
        let lines = render::load_and_render(source, &self.config, term)?;
        for line in &lines {
            output.write_line(line)?;
        }
        Ok(())
    }
}

pub struct SvgViewer {
    config: ImageConfig,
}

impl SvgViewer {
    pub fn new(config: ImageConfig) -> Self {
        Self { config }
    }
}

impl Viewer for SvgViewer {
    fn render(
        &self,
        source: &InputSource,
        _file_type: &FileType,
        output: &mut Output,
    ) -> Result<()> {
        let term = render::TermSize::detect();
        let lines = render::load_and_render_svg(source, &self.config, term)?;
        for line in &lines {
            output.write_line(line)?;
        }
        Ok(())
    }
}
