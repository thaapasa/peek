use std::fs;
use std::path::Path;

use anyhow::Result;
use syntect::easy::HighlightLines;
use syntect::util::as_24_bit_terminal_escaped;

use crate::detect::FileType;
use crate::pager::Output;
use crate::theme::ThemeManager;

use super::Viewer;

pub struct SyntaxViewer {
    theme: ThemeManager,
    forced_language: Option<String>,
}

impl SyntaxViewer {
    pub fn new(theme: ThemeManager, forced_language: Option<String>) -> Self {
        Self {
            theme,
            forced_language,
        }
    }

    fn find_syntax_name(&self, path: &Path, file_type: &FileType) -> Option<String> {
        // User-forced language takes priority
        if let Some(ref lang) = self.forced_language {
            return Some(lang.clone());
        }

        // Use the detected syntax hint from file type
        if let FileType::SourceCode {
            syntax: Some(ext),
        } = file_type
        {
            return Some(ext.clone());
        }

        // Try the full filename (for things like Makefile, Dockerfile)
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    }
}

impl Viewer for SyntaxViewer {
    fn render(&self, path: &Path, file_type: &FileType, output: &mut Output) -> Result<()> {
        let content = fs::read_to_string(path)?;

        let syntax_name = self.find_syntax_name(path, file_type);

        // Try to find a matching syntax definition
        let syntax = syntax_name
            .and_then(|name| {
                self.theme
                    .syntax_set
                    .find_syntax_by_token(&name)
                    .or_else(|| self.theme.syntax_set.find_syntax_by_extension(&name))
            })
            .unwrap_or_else(|| self.theme.syntax_set.find_syntax_plain_text());

        let theme = self.theme.theme();
        let mut highlighter = HighlightLines::new(syntax, theme);

        for line in content.lines() {
            let ranges = highlighter.highlight_line(line, &self.theme.syntax_set)?;
            let escaped = as_24_bit_terminal_escaped(&ranges, false);
            output.write_line(&format!("{escaped}\x1b[0m"))?;
        }

        Ok(())
    }
}
