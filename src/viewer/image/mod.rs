use std::io::Write;
use std::path::Path;

use anyhow::Result;

use crate::detect::FileType;
use crate::pager::Output;
use crate::theme::PeekTheme;

use super::Viewer;

mod clustering;
mod glyph_atlas;
pub mod render;

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

pub struct ImageViewer {
    width: u32,
    mode: ImageMode,
    peek_theme: PeekTheme,
}

impl ImageViewer {
    pub fn new(width: u32, mode: ImageMode, peek_theme: PeekTheme) -> Self {
        Self { width, mode, peek_theme }
    }

    /// Interactive image viewing with resize support.
    /// Enters alternate screen and blocks until the user quits.
    pub fn view_interactive(&self, path: &Path, file_type: &FileType) -> Result<()> {
        let mode = self.mode;
        let width = self.width;
        super::interactive::view_interactive(path, file_type, &self.peek_theme, |stdout| {
            let term = render::TermSize::detect();
            let lines = render::load_and_render(path, mode, width, term)?;
            for line in &lines {
                stdout.write_all(line.as_bytes())?;
                stdout.write_all(b"\r\n")?;
            }
            Ok(())
        })
    }
}

impl Viewer for ImageViewer {
    fn render(&self, path: &Path, _file_type: &FileType, output: &mut Output) -> Result<()> {
        let term = render::TermSize::detect();
        let lines = render::load_and_render(path, self.mode, self.width, term)?;
        for line in &lines {
            output.write_line(line)?;
        }
        Ok(())
    }
}
