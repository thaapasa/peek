use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use anyhow::{Result, bail};

use crate::detect::FileType;
use crate::input::InputSource;
use crate::pager::Output;
use crate::theme::{PeekThemeName, ThemeManager};

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

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Black => "black",
            Self::White => "white",
            Self::Checkerboard => "checker",
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
}

/// Shared configuration for image rendering (mode, size, background, margin).
#[derive(Debug, Clone, Copy)]
pub struct ImageConfig {
    pub mode: ImageMode,
    pub width: u32,
    pub background: Background,
    pub margin: u32,
}

/// Extract a filesystem path from an input source, bailing for stdin.
/// Image rendering currently requires a path (animation detection and
/// SVG rasterization both open files directly).
fn require_path<'a>(source: &'a InputSource, kind: &str) -> Result<&'a Path> {
    match source.path() {
        Some(p) => Ok(p),
        None => bail!("{kind} rendering from stdin is not yet supported"),
    }
}

pub struct ImageViewer {
    config: ImageConfig,
    theme_name: PeekThemeName,
}

impl ImageViewer {
    pub fn new(config: ImageConfig, theme_name: PeekThemeName) -> Self {
        Self { config, theme_name }
    }

    /// Interactive image viewing with resize support.
    /// Enters alternate screen and blocks until the user quits.
    pub fn view_interactive(&self, source: &InputSource, file_type: &FileType) -> Result<()> {
        let path = require_path(source, "image")?;

        // Check for animated image (GIF/WebP) — use dedicated animation viewer
        if let Some(frames) = animate::decode_anim_frames(path)? {
            return animate::view_animated(
                source, file_type, frames, self.config, self.theme_name,
            );
        }

        let config = self.config;
        let bg = Rc::new(Cell::new(config.background));
        let bg_closure = Rc::clone(&bg);
        let path_buf = path.to_path_buf();
        super::interactive::view_interactive_with_bg(
            source, file_type, self.theme_name, true, true,
            Some(bg),
            move |_theme, _pretty| {
                let mut term = render::TermSize::detect();
                term.rows = term.rows.saturating_sub(1);
                render::load_and_render(&path_buf, config.mode, config.width, term, bg_closure.get(), config.margin)
            },
        )
    }
}

impl Viewer for ImageViewer {
    fn render(
        &self,
        source: &InputSource,
        _file_type: &FileType,
        output: &mut Output,
    ) -> Result<()> {
        let path = require_path(source, "image")?;
        let term = render::TermSize::detect();
        let c = &self.config;
        let lines = render::load_and_render(path, c.mode, c.width, term, c.background, c.margin)?;
        for line in &lines {
            output.write_line(line)?;
        }
        Ok(())
    }
}

pub struct SvgViewer {
    config: ImageConfig,
    theme_name: PeekThemeName,
    theme_manager: Rc<ThemeManager>,
    raw_mode: bool,
}

impl SvgViewer {
    pub fn new(
        config: ImageConfig,
        theme_name: PeekThemeName,
        theme_manager: Rc<ThemeManager>,
        raw_mode: bool,
    ) -> Self {
        Self { config, theme_name, theme_manager, raw_mode }
    }

    pub fn view_interactive(&self, source: &InputSource, file_type: &FileType) -> Result<()> {
        let path = require_path(source, "SVG")?;

        let config = self.config;
        let bg = Rc::new(Cell::new(config.background));
        let bg_closure = Rc::clone(&bg);
        let path_buf = path.to_path_buf();
        let tm = Rc::clone(&self.theme_manager);
        let raw_mode = self.raw_mode;

        super::interactive::view_interactive_with_bg(
            source,
            file_type,
            self.theme_name,
            true,
            true, // start with pretty=true (image preview)
            Some(bg),
            move |theme_name, pretty| {
                if pretty {
                    // Render as ASCII art image
                    let mut term = render::TermSize::detect();
                    term.rows = term.rows.saturating_sub(1);
                    render::load_and_render_svg(&path_buf, config.mode, config.width, term, bg_closure.get(), config.margin)
                } else {
                    // Render as XML source
                    let raw_content = std::fs::read_to_string(&path_buf)?;
                    let content = if !raw_mode {
                        crate::viewer::structured::pretty_print(
                            &raw_content,
                            crate::detect::StructuredFormat::Xml,
                        )?
                    } else {
                        raw_content
                    };
                    super::highlight_lines(&content, "XML", &tm, theme_name)
                }
            },
        )
    }
}

impl Viewer for SvgViewer {
    fn render(
        &self,
        source: &InputSource,
        _file_type: &FileType,
        output: &mut Output,
    ) -> Result<()> {
        let path = require_path(source, "SVG")?;
        let term = render::TermSize::detect();
        let c = &self.config;
        let lines = render::load_and_render_svg(path, c.mode, c.width, term, c.background, c.margin)?;
        for line in &lines {
            output.write_line(line)?;
        }
        Ok(())
    }
}
