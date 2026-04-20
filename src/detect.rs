use std::fs;
use std::path::Path;

use anyhow::{Result, bail};

use crate::input::InputSource;

/// Detected file type, used to dispatch to the right viewer.
#[derive(Debug, Clone)]
pub enum FileType {
    /// Source code or text file with optional syntax name
    SourceCode { syntax: Option<String> },
    /// Structured data format
    Structured(StructuredFormat),
    /// Raster image
    Image,
    /// SVG vector image (rasterized for preview, XML source for raw view)
    Svg,
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

/// Detect the file type of an input source.
pub fn detect(source: &InputSource) -> Result<FileType> {
    match source {
        InputSource::File(path) => detect_file(path),
        InputSource::Stdin { data } => Ok(detect_bytes(data)),
    }
}

fn detect_file(path: &Path) -> Result<FileType> {
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
            "svg" => {
                return Ok(FileType::Svg);
            }
            "xml" | "html" | "htm" | "xhtml" | "plist" => {
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

/// Detect the file type from an in-memory byte buffer (for stdin).
/// Uses magic bytes for binary formats, then content sniffing for text.
fn detect_bytes(data: &[u8]) -> FileType {
    // Magic-byte detection
    if let Some(kind) = infer::get(data) {
        let mime = kind.mime_type();
        if mime == "image/svg+xml" {
            return FileType::Svg;
        }
        if mime.starts_with("image/") {
            return FileType::Image;
        }
        if mime.starts_with("video/")
            || mime.starts_with("audio/")
            || mime.starts_with("application/zip")
            || mime.starts_with("application/gzip")
            || mime.starts_with("application/x-executable")
        {
            return FileType::Binary;
        }
    }

    // Non-UTF-8 → binary
    let Ok(text) = std::str::from_utf8(data) else {
        return FileType::Binary;
    };

    // Content-based format sniffing
    let trimmed = text.trim_start();
    let first = trimmed.as_bytes().first().copied();

    match first {
        Some(b'{') | Some(b'[') => {
            if serde_json::from_str::<serde_json::Value>(text).is_ok() {
                return FileType::Structured(StructuredFormat::Json);
            }
        }
        Some(b'<') => {
            // SVG has a distinctive root element — catch it before generic XML
            if trimmed.contains("<svg") {
                return FileType::Svg;
            }
            return FileType::Structured(StructuredFormat::Xml);
        }
        _ => {}
    }

    // YAML document marker or directive
    if trimmed.starts_with("---\n")
        || trimmed.starts_with("---\r\n")
        || trimmed == "---"
        || trimmed.starts_with("%YAML")
    {
        return FileType::Structured(StructuredFormat::Yaml);
    }

    // Plain text — the SyntaxViewer's `--language` flag can still apply
    FileType::SourceCode { syntax: None }
}
