use std::fs;
use std::path::Path;

use anyhow::{bail, Result};

/// Detected file type, used to dispatch to the right viewer.
#[derive(Debug, Clone)]
pub enum FileType {
    /// Source code or text file with optional syntax name
    SourceCode { syntax: Option<String> },
    /// Structured data format
    Structured(StructuredFormat),
    /// Raster image
    Image,
    /// Binary / unknown
    Binary,
}

#[derive(Debug, Clone, Copy)]
pub enum StructuredFormat {
    Json,
    Yaml,
    Toml,
    Xml,
}

/// Detect the file type by extension and magic bytes.
pub fn detect(path: &Path) -> Result<FileType> {
    if !path.exists() {
        bail!("file not found: {}", path.display());
    }

    // Check extension first for structured formats
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        match ext.to_lowercase().as_str() {
            "json" | "geojson" | "jsonl" => {
                return Ok(FileType::Structured(StructuredFormat::Json));
            }
            "yaml" | "yml" => {
                return Ok(FileType::Structured(StructuredFormat::Yaml));
            }
            "toml" => {
                return Ok(FileType::Structured(StructuredFormat::Toml));
            }
            "xml" | "svg" | "html" | "htm" | "xhtml" | "plist" => {
                return Ok(FileType::Structured(StructuredFormat::Xml));
            }
            _ => {}
        }
    }

    // Check magic bytes for images and binary files
    let buf = fs::read(path)?;
    if let Some(kind) = infer::get(&buf) {
        let mime = kind.mime_type();
        if mime.starts_with("image/") {
            return Ok(FileType::Image);
        }
        // Known binary types that aren't text
        if mime.starts_with("video/")
            || mime.starts_with("audio/")
            || mime.starts_with("application/zip")
            || mime.starts_with("application/gzip")
            || mime.starts_with("application/x-executable")
        {
            return Ok(FileType::Binary);
        }
    }

    // If the file has significant non-UTF-8 content, treat as binary
    let is_text = String::from_utf8(buf).is_ok();
    if !is_text {
        return Ok(FileType::Binary);
    }

    // It's a text file — use extension as syntax hint
    let syntax = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());

    Ok(FileType::SourceCode { syntax })
}
