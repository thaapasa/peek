use std::fs;
use std::path::Path;
use std::rc::Rc;

use anyhow::Result;
use syntect::easy::HighlightLines;
use syntect::util::as_24_bit_terminal_escaped;

use crate::detect::{FileType, StructuredFormat};
use crate::pager::Output;
use crate::theme::ThemeManager;

use super::Viewer;

pub struct StructuredViewer {
    theme: Rc<ThemeManager>,
}

impl StructuredViewer {
    pub fn new(theme: Rc<ThemeManager>) -> Self {
        Self { theme }
    }
}

impl Viewer for StructuredViewer {
    fn render(&self, path: &Path, file_type: &FileType, output: &mut Output) -> Result<()> {
        let format = match file_type {
            FileType::Structured(f) => *f,
            _ => return Ok(()),
        };

        let raw = fs::read_to_string(path)?;

        let pretty = match format {
            StructuredFormat::Json => pretty_json(&raw)?,
            StructuredFormat::Yaml => pretty_yaml(&raw)?,
            StructuredFormat::Toml => pretty_toml(&raw)?,
            StructuredFormat::Xml => pretty_xml(&raw),
        };

        // Syntax-highlight the pretty-printed output
        let syntax_name = match format {
            StructuredFormat::Json => "JSON",
            StructuredFormat::Yaml => "YAML",
            StructuredFormat::Toml => "TOML",
            StructuredFormat::Xml => "XML",
        };

        let syntax = self
            .theme
            .syntax_set
            .find_syntax_by_name(syntax_name)
            .unwrap_or_else(|| self.theme.syntax_set.find_syntax_plain_text());
        let theme = self.theme.theme();
        let mut highlighter = HighlightLines::new(syntax, theme);

        for line in pretty.lines() {
            let ranges = highlighter.highlight_line(line, &self.theme.syntax_set)?;
            let escaped = as_24_bit_terminal_escaped(&ranges, false);
            output.write_line(&format!("{escaped}\x1b[0m"))?;
        }

        Ok(())
    }
}

/// Pretty-print a structured document.
pub fn pretty_print(raw: &str, format: StructuredFormat) -> Result<String> {
    match format {
        StructuredFormat::Json => pretty_json(raw),
        StructuredFormat::Yaml => pretty_yaml(raw),
        StructuredFormat::Toml => pretty_toml(raw),
        StructuredFormat::Xml => Ok(pretty_xml(raw)),
    }
}

fn pretty_json(raw: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    Ok(serde_json::to_string_pretty(&value)?)
}

fn pretty_yaml(raw: &str) -> Result<String> {
    let value: serde_yaml::Value = serde_yaml::from_str(raw)?;
    Ok(serde_yaml::to_string(&value)?)
}

fn pretty_toml(raw: &str) -> Result<String> {
    let value: toml::Value = toml::from_str(raw)?;
    Ok(toml::to_string_pretty(&value)?)
}

fn pretty_xml(raw: &str) -> String {
    // Simple indentation pass for XML
    let mut result = String::new();
    let mut depth: usize = 0;
    let indent = "  ";

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Decrease depth for closing tags
        if trimmed.starts_with("</") {
            depth = depth.saturating_sub(1);
        }

        for _ in 0..depth {
            result.push_str(indent);
        }
        result.push_str(trimmed);
        result.push('\n');

        // Increase depth for opening tags (not self-closing, not closing)
        if trimmed.starts_with('<')
            && !trimmed.starts_with("</")
            && !trimmed.starts_with("<!")
            && !trimmed.starts_with("<?")
            && !trimmed.ends_with("/>")
            && trimmed.ends_with('>')
        {
            depth += 1;
        }
    }

    result
}
