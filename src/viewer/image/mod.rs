use std::path::Path;

use anyhow::Result;

use crate::detect::FileType;
use crate::pager::Output;
use crate::theme::PeekThemeName;

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
    theme_name: PeekThemeName,
}

impl ImageViewer {
    pub fn new(width: u32, mode: ImageMode, theme_name: PeekThemeName) -> Self {
        Self { width, mode, theme_name }
    }

    /// Interactive image viewing with resize support.
    /// Enters alternate screen and blocks until the user quits.
    pub fn view_interactive(&self, path: &Path, file_type: &FileType) -> Result<()> {
        let mode = self.mode;
        let width = self.width;
        let path = path.to_path_buf();
        super::interactive::view_interactive(&path, file_type, self.theme_name, true, true, |_theme, _pretty| {
            let mut term = render::TermSize::detect();
            term.rows = term.rows.saturating_sub(1); // reserve row for status line
            render::load_and_render(&path, mode, width, term)
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
