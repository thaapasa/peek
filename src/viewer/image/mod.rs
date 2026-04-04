use std::path::Path;

use anyhow::Result;

use crate::detect::FileType;
use crate::pager::Output;

use super::Viewer;

mod clustering;
mod glyph_atlas;
mod interactive;
mod render;

/// Image rendering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMode {
    /// All glyphs: ASCII + extended + block elements + box drawing + geometric shapes
    Full,
    /// Unicode block/quadrant elements + curated ASCII subset
    Block,
    /// Legacy density-ramp renderer (brightness-based, foreground only)
    Ascii,
}

impl ImageMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "block" => Self::Block,
            "ascii" => Self::Ascii,
            _ => Self::Full,
        }
    }
}

pub struct ImageViewer {
    width: u32,
    mode: ImageMode,
}

impl ImageViewer {
    pub fn new(width: u32, mode: ImageMode) -> Self {
        Self { width, mode }
    }

    /// Interactive image viewing with resize support.
    /// Enters alternate screen and blocks until the user quits.
    pub fn view_interactive(&self, path: &Path) -> Result<()> {
        interactive::view_interactive(path, self.mode, self.width)
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
