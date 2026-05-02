use std::rc::Rc;

use anyhow::Result;
use syntect::easy::HighlightLines;

use crate::input::InputSource;
use crate::input::detect::{FileType, StructuredFormat};
use crate::output::PrintOutput;
use crate::theme::ThemeManager;

use super::{Viewer, ranges_to_escaped};

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
        output: &mut PrintOutput,
    ) -> Result<()> {
        let format = match file_type {
            FileType::Structured(f) => *f,
            _ => unreachable!("registry only routes Structured here"),
        };

        let raw = source.read_text()?;

        // On parse failure fall back to the raw source (and warn on stderr)
        // rather than aborting the whole viewer — same behavior the
        // interactive ContentMode now has.
        let pretty = if self.raw_mode {
            raw.clone()
        } else {
            let result = match format {
                StructuredFormat::Json => pretty_json(&raw),
                StructuredFormat::Yaml => pretty_yaml(&raw),
                StructuredFormat::Toml => pretty_toml(&raw),
                StructuredFormat::Xml => pretty_xml(&raw),
            };
            match result {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "warning: {} parse failed for {} ({e}); showing raw source",
                        format_label(format),
                        source.name(),
                    );
                    raw.clone()
                }
            }
        };

        // Syntax-highlight the pretty-printed output
        let syntax_name = format_label(format);

        let syntax = self
            .theme
            .syntax_set
            .find_syntax_by_name(syntax_name)
            .unwrap_or_else(|| self.theme.syntax_set.find_syntax_plain_text());
        let theme = self.theme.theme();
        let mut highlighter = HighlightLines::new(syntax, theme);

        let color_mode = self.theme.color_mode();
        for line in pretty.lines() {
            let ranges = highlighter.highlight_line(line, &self.theme.syntax_set)?;
            output.write_line(&ranges_to_escaped(&ranges, color_mode))?;
        }

        Ok(())
    }
}

/// Display name for a structured format — also serves as the syntect
/// syntax token (`"JSON"`, `"YAML"`, etc.).
fn format_label(format: StructuredFormat) -> &'static str {
    match format {
        StructuredFormat::Json => "JSON",
        StructuredFormat::Yaml => "YAML",
        StructuredFormat::Toml => "TOML",
        StructuredFormat::Xml => "XML",
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
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    use quick_xml::writer::Writer;
    use std::io::Cursor;

    // Don't trim text content — for HTML/XHTML this would collapse <pre>
    // blocks and inline whitespace between tags. We keep the document
    // semantically intact at the cost of a slightly less compact output.
    let mut reader = Reader::from_str(raw);
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            event => writer.write_event(event)?,
        }
    }

    Ok(String::from_utf8(writer.into_inner().into_inner())?)
}
