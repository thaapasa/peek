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
    raw_mode: bool,
}

impl StructuredViewer {
    pub fn new(theme: Rc<ThemeManager>, raw_mode: bool) -> Self {
        Self { theme, raw_mode }
    }
}

impl Viewer for StructuredViewer {
    fn render(&self, path: &Path, file_type: &FileType, output: &mut Output) -> Result<()> {
        let format = match file_type {
            FileType::Structured(f) => *f,
            _ => return Ok(()),
        };

        let raw = fs::read_to_string(path)?;

        let pretty = if self.raw_mode {
            raw.clone()
        } else {
            match format {
                StructuredFormat::Json => pretty_json(&raw)?,
                StructuredFormat::Yaml => pretty_yaml(&raw)?,
                StructuredFormat::Toml => pretty_toml(&raw)?,
                StructuredFormat::Xml => pretty_xml(&raw),
            }
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
    // Split at tag boundaries so each tag gets its own line,
    // then indent based on nesting depth.
    let split = raw.replace("><", ">\n<");

    let mut result = String::new();
    let mut depth: usize = 0;
    let indent = "  ";

    for line in split.lines() {
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

        // Increase depth for opening tags, but not if:
        // - self-closing (/>), closing (</), declaration (<!), processing (<?)
        // - contains its own closing tag (<tag>text</tag>)
        // - is an HTML void element (<meta>, <br>, <img>, etc.)
        if trimmed.starts_with('<')
            && !trimmed.starts_with("</")
            && !trimmed.starts_with("<!")
            && !trimmed.starts_with("<?")
            && !trimmed.ends_with("/>")
            && trimmed.ends_with('>')
            && !has_inline_close(trimmed)
            && !is_void_element(trimmed)
        {
            depth += 1;
        }
    }

    result
}

/// Check if a line like `<tag>content</tag>` contains its own closing tag.
fn has_inline_close(line: &str) -> bool {
    match line.find('>') {
        Some(pos) => line[pos + 1..].contains("</"),
        None => false,
    }
}

/// Check if the tag is an HTML void element (self-closing without `/>` suffix).
fn is_void_element(line: &str) -> bool {
    const VOID_TAGS: &[&str] = &[
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ];
    let tag = line.trim_start_matches('<');
    let end = tag
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(tag.len());
    let name = tag[..end].to_lowercase();
    VOID_TAGS.iter().any(|&t| t == name)
}
