use std::rc::Rc;

use anyhow::Result;

use crate::input::detect::FileType;
use crate::input::InputSource;
use crate::output::Output;
use crate::theme::ThemeManager;

use super::{syntax_token_for, Viewer};

pub struct SyntaxViewer {
    theme: Rc<ThemeManager>,
    forced_language: Option<String>,
}

impl SyntaxViewer {
    pub fn new(theme: Rc<ThemeManager>, forced_language: Option<String>) -> Self {
        Self {
            theme,
            forced_language,
        }
    }
}

impl Viewer for SyntaxViewer {
    fn render(
        &self,
        source: &InputSource,
        file_type: &FileType,
        output: &mut Output,
    ) -> Result<()> {
        let content = source.read_text()?;

        let lines = if let Some(token) =
            syntax_token_for(self.forced_language.as_deref(), source, file_type)
        {
            super::highlight_lines(
                &content,
                &token,
                &self.theme,
                self.theme.theme_name,
                self.theme.color_mode(),
            )?
        } else {
            content.lines().map(String::from).collect()
        };

        for line in &lines {
            output.write_line(line)?;
        }

        Ok(())
    }
}
