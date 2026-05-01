//! Piped-output viewer for raster images. Decodes the source, renders it
//! through the glyph-matched ASCII art pipeline, and writes the resulting
//! lines to the [`Output`].

use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::FileType;
use crate::output::Output;

use super::Viewer;
use super::{ImageConfig, render};

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
