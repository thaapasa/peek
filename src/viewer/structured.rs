use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::detect::{FileType, StructuredFormat};
use crate::pager::Output;

use super::Viewer;

pub struct StructuredViewer;

impl StructuredViewer {
    pub fn new() -> Self {
        Self
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

        // The pretty-printed output is plain text for now.
        // TODO: apply syntax highlighting on top of the formatted output
        output.write_str(&pretty)?;

        Ok(())
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
