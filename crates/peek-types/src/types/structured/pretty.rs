//! Structured-data pretty-printers for JSON, YAML, TOML, and XML.
//!
//! `pretty_print` is called from `ContentMode::ensure_pretty` to lazily
//! reflow the source the first time the user lands on the pretty view
//! (or, in pipe mode, when the primary mode renders).

use anyhow::Result;

use crate::input::detect::StructuredFormat;

/// Pretty-print a structured document.
pub fn pretty_print(raw: &str, format: StructuredFormat) -> Result<String> {
    match format {
        StructuredFormat::Json => pretty_json(raw),
        StructuredFormat::Jsonc => pretty_jsonc(raw),
        StructuredFormat::Json5 => pretty_json5(raw),
        StructuredFormat::Jsonl => pretty_jsonl(raw),
        StructuredFormat::Yaml => pretty_yaml(raw),
        StructuredFormat::Toml => pretty_toml(raw),
        StructuredFormat::Xml => pretty_xml(raw),
    }
}

fn pretty_json(raw: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    Ok(serde_json::to_string_pretty(&value)?)
}

/// Pretty path for JSONC strips comments before reformatting. Comments
/// don't survive — `r` (raw) keeps the original source if they matter.
fn pretty_jsonc(raw: &str) -> Result<String> {
    let stripped = super::info::strip_json_comments(raw);
    pretty_json(&stripped)
}

/// Pretty path for JSON5 round-trips through serde_json, normalising to
/// strict JSON output. Single quotes / unquoted keys / trailing commas /
/// hex literals all collapse — by design; raw view preserves the source.
fn pretty_json5(raw: &str) -> Result<String> {
    let value: serde_json::Value = json5::from_str(raw)?;
    Ok(serde_json::to_string_pretty(&value)?)
}

/// Pretty path for JSON Lines: pretty-print every non-empty line and
/// separate the documents with a blank line so they're visually distinct.
fn pretty_jsonl(raw: &str) -> Result<String> {
    let mut out = String::with_capacity(raw.len() * 2);
    for (i, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(trimmed)?;
        if i > 0 && !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&serde_json::to_string_pretty(&value)?);
        out.push('\n');
    }
    Ok(out)
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
