use std::path::Path;

use anyhow::Result;

use crate::detect::FileType;
use crate::pager::Output;

use super::Viewer;

mod block_color;
mod clustering;
mod density;
mod glyph_atlas;

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
}

impl Viewer for ImageViewer {
    fn render(&self, path: &Path, file_type: &FileType, output: &mut Output) -> Result<()> {
        match self.mode {
            ImageMode::Ascii => {
                let renderer = density::DensityRenderer::new(self.width);
                renderer.render(path, file_type, output)
            }
            ImageMode::Full | ImageMode::Block => {
                let renderer =
                    block_color::BlockColorRenderer::new(self.width, self.mode);
                renderer.render(path, file_type, output)
            }
        }
    }
}
