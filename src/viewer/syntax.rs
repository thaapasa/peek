use std::fs;
use std::path::Path;
use std::rc::Rc;

use anyhow::Result;

use crate::detect::FileType;
use crate::pager::Output;
use crate::theme::ThemeManager;

use super::Viewer;

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

        let lines = if let Some(token) = self.find_syntax_name(path, file_type) {
            super::highlight_lines(&content, &token, &self.theme, self.theme.theme_name)?
        } else {
            content.lines().map(String::from).collect()
        };

        for line in &lines {
            output.write_line(line)?;
        }

        Ok(())
    }
}
