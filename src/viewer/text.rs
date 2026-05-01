use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::FileType;
use crate::output::Output;

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
