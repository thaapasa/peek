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

/// Result of file-type detection. Carries the magic-byte MIME forward so
/// `info::gather` doesn't need to re-read the file and re-run `infer`.
#[derive(Debug, Clone)]
pub struct Detected {
    pub file_type: FileType,
    /// MIME type from `infer::get` magic-byte detection. `None` when the
    /// file's leading bytes don't match any format `infer` recognizes
    /// (true for plain-text source code, structured text files, etc.).
    pub magic_mime: Option<String>,
}

/// Detect the file type of an input source.
pub fn detect(source: &InputSource) -> Result<Detected> {
    match source {
        InputSource::File(path) => detect_file(path),
        InputSource::Stdin { data } => Ok(detect_bytes(data)),
    }
}

fn detect_file(path: &Path) -> Result<Detected> {
    if !path.exists() {
        bail!("file not found: {}", path.display());
    }

    // Check extension first for structured formats
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let file_type = match ext.to_lowercase().as_str() {
            "json" | "geojson" | "jsonl" => Some(FileType::Structured(StructuredFormat::Json)),
            "yaml" | "yml" => Some(FileType::Structured(StructuredFormat::Yaml)),
            "toml" => Some(FileType::Structured(StructuredFormat::Toml)),
            "svg" => Some(FileType::Svg),
            "xml" | "html" | "htm" | "xhtml" | "plist" => {
                Some(FileType::Structured(StructuredFormat::Xml))
            }
            _ => None,
        };
        if let Some(file_type) = file_type {
            return Ok(Detected { file_type, magic_mime: None });
        }
    }

    // Check magic bytes for images and binary files
    let buf = fs::read(path)?;
    let magic_mime = infer::get(&buf).map(|k| k.mime_type().to_string());
    if let Some(ref mime) = magic_mime {
        if mime.starts_with("image/") {
            return Ok(Detected {
                file_type: FileType::Image,
                magic_mime,
            });
        }
        // Known binary types that aren't text
        if mime.starts_with("video/")
            || mime.starts_with("audio/")
            || mime.starts_with("application/zip")
            || mime.starts_with("application/gzip")
            || mime.starts_with("application/x-executable")
        {
            return Ok(Detected {
                file_type: FileType::Binary,
                magic_mime,
            });
        }
    }

    // If the file has significant non-UTF-8 content, treat as binary
    let is_text = String::from_utf8(buf).is_ok();
    if !is_text {
        return Ok(Detected {
            file_type: FileType::Binary,
            magic_mime,
        });
    }

    // It's a text file — use extension as syntax hint
    let syntax = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());

    Ok(Detected {
        file_type: FileType::SourceCode { syntax },
        magic_mime,
    })
}

/// Detect the file type from an in-memory byte buffer (for stdin).
/// Uses magic bytes for binary formats, then content sniffing for text.
fn detect_bytes(data: &[u8]) -> Detected {
    let magic_mime = infer::get(data).map(|k| k.mime_type().to_string());
    if let Some(ref mime) = magic_mime {
        if mime == "image/svg+xml" {
            return Detected { file_type: FileType::Svg, magic_mime };
        }
        if mime.starts_with("image/") {
            return Detected { file_type: FileType::Image, magic_mime };
        }
        if mime.starts_with("video/")
            || mime.starts_with("audio/")
            || mime.starts_with("application/zip")
            || mime.starts_with("application/gzip")
            || mime.starts_with("application/x-executable")
        {
            return Detected { file_type: FileType::Binary, magic_mime };
        }
    }

    // Non-UTF-8 → binary
    let Ok(text) = std::str::from_utf8(data) else {
        return Detected {
            file_type: FileType::Binary,
            magic_mime,
        };
    };

    // Content-based format sniffing
    let trimmed = text.trim_start();
    let first = trimmed.as_bytes().first().copied();

    match first {
        Some(b'{') | Some(b'[') => {
            if serde_json::from_str::<serde_json::Value>(text).is_ok() {
                return Detected {
                    file_type: FileType::Structured(StructuredFormat::Json),
                    magic_mime,
                };
            }
        }
        Some(b'<') => {
            // SVG has a distinctive root element — catch it before generic XML
            if trimmed.contains("<svg") {
                return Detected { file_type: FileType::Svg, magic_mime };
            }
            return Detected {
                file_type: FileType::Structured(StructuredFormat::Xml),
                magic_mime,
            };
        }
        _ => {}
    }

    // YAML document marker or directive
    if trimmed.starts_with("---\n")
        || trimmed.starts_with("---\r\n")
        || trimmed == "---"
        || trimmed.starts_with("%YAML")
    {
        return Detected {
            file_type: FileType::Structured(StructuredFormat::Yaml),
            magic_mime,
        };
    }

    // Plain text — the SyntaxViewer's `--language` flag can still apply
    Detected {
        file_type: FileType::SourceCode { syntax: None },
        magic_mime,
    }
}
