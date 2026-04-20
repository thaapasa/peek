use std::rc::Rc;

use anyhow::Result;
use syntect::easy::HighlightLines;
use syntect::util::as_24_bit_terminal_escaped;

use crate::detect::{FileType, StructuredFormat};
use crate::input::InputSource;
use crate::pager::Output;
use crate::theme::{ANSI_RESET, ThemeManager};

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
    fn render(
        &self,
        source: &InputSource,
        file_type: &FileType,
        output: &mut Output,
    ) -> Result<()> {
        let format = match file_type {
            FileType::Structured(f) => *f,
            _ => return Ok(()),
        };

        let raw = source.read_text()?;

        let pretty = if self.raw_mode {
            raw.clone()
        } else {
            match format {
                StructuredFormat::Json => pretty_json(&raw)?,
                StructuredFormat::Yaml => pretty_yaml(&raw)?,
                StructuredFormat::Toml => pretty_toml(&raw)?,
                StructuredFormat::Xml => pretty_xml(&raw)?,
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
            output.write_line(&format!("{escaped}{ANSI_RESET}"))?;
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
        StructuredFormat::Xml => pretty_xml(raw),
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

fn pretty_xml(raw: &str) -> Result<String> {
    use std::io::Cursor;
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    use quick_xml::writer::Writer;

    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            event => writer.write_event(event)?,
        }
    }

    Ok(String::from_utf8(writer.into_inner().into_inner())?)
}
