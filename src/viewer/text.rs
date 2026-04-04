use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::detect::FileType;
use crate::pager::Output;

use super::Viewer;

/// Plain text viewer — just reads the file and writes it out.
pub struct TextViewer;

impl Viewer for TextViewer {
    fn render(&self, path: &Path, _file_type: &FileType, output: &mut Output) -> Result<()> {
        let content = fs::read_to_string(path)?;
        output.write_str(&content)?;
        Ok(())
    }
}
