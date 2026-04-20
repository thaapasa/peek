use anyhow::Result;

use crate::detect::FileType;
use crate::input::InputSource;
use crate::pager::Output;

use super::Viewer;

/// Plain text viewer — just reads the content and writes it out.
pub struct TextViewer;

impl Viewer for TextViewer {
    fn render(
        &self,
        source: &InputSource,
        _file_type: &FileType,
        output: &mut Output,
    ) -> Result<()> {
        let content = source.read_text()?;
        output.write_str(&content)?;
        Ok(())
    }
}
