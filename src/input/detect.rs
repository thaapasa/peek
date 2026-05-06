use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::{Result, bail};

use crate::input::InputSource;

/// Bytes read from the head of a file for magic-byte detection. `infer`
/// inspects only the first few hundred bytes; 16 KB is comfortable headroom.
const HEAD_BYTES: usize = 16 * 1024;

/// Chunk size for streaming UTF-8 validation of the file body.
const SCAN_CHUNK: usize = 64 * 1024;

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
    /// Container archive (zip / tar / compressed tar). Drives the
    /// listing-only TOC viewer — no payload decompression.
    Archive(ArchiveFormat),
    /// Disk image (ISO / DMG / etc). Drives a metadata-only info view —
    /// volume descriptor / trailer parsing, no filesystem walk.
    DiskImage(DiskImageFormat),
    /// Binary / unknown
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredFormat {
    Json,
    /// JSON with comments (VS Code flavor): `//` and `/* … */` allowed.
    Jsonc,
    /// JSON5: comments, unquoted keys, trailing commas, single quotes, hex.
    Json5,
    /// JSON Lines / NDJSON: one JSON value per line.
    Jsonl,
    Yaml,
    Toml,
    Xml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    SevenZ,
}

impl ArchiveFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP archive",
            Self::Tar => "tar archive",
            Self::TarGz => "tar + gzip",
            Self::TarBz2 => "tar + bzip2",
            Self::TarXz => "tar + xz",
            Self::TarZst => "tar + zstd",
            Self::SevenZ => "7-Zip archive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskImageFormat {
    Iso,
}

impl DiskImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Iso => "ISO 9660 image",
        }
    }
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

    // Archive double-extensions (.tar.gz, .tgz, etc.) check the full file
    // name, so they win over the single-extension fallback below for files
    // like `archive.tar.gz` where `extension()` would only see `.gz`.
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && let Some(fmt) = archive_format_from_name(name)
    {
        return Ok(Detected {
            file_type: FileType::Archive(fmt),
            magic_mime: None,
        });
    }

    // Disk-image extensions resolve before the structured/text fallback so
    // the single-extension match below doesn't ever see them.
    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && let Some(fmt) = disk_image_format_from_ext(&ext.to_lowercase())
    {
        return Ok(Detected {
            file_type: FileType::DiskImage(fmt),
            magic_mime: None,
        });
    }

    // Check extension first for structured formats
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let file_type = match ext.to_lowercase().as_str() {
            "json" | "geojson" => Some(FileType::Structured(StructuredFormat::Json)),
            "jsonc" => Some(FileType::Structured(StructuredFormat::Jsonc)),
            "json5" => Some(FileType::Structured(StructuredFormat::Json5)),
            "jsonl" | "ndjson" => Some(FileType::Structured(StructuredFormat::Jsonl)),
            "yaml" | "yml" => Some(FileType::Structured(StructuredFormat::Yaml)),
            "toml" => Some(FileType::Structured(StructuredFormat::Toml)),
            "svg" => Some(FileType::Svg),
            "xml" | "html" | "htm" | "xhtml" | "plist" => {
                Some(FileType::Structured(StructuredFormat::Xml))
            }
            _ => None,
        };
        if let Some(file_type) = file_type {
            return Ok(Detected {
                file_type,
                magic_mime: None,
            });
        }
    }

    // Read just the head for magic-byte detection — `infer` only inspects
    // the first few hundred bytes, so we never need the whole file.
    let mut file = fs::File::open(path)?;
    let mut head = vec![0u8; HEAD_BYTES];
    let n = read_fill(&mut file, &mut head)?;
    head.truncate(n);

    let magic_mime = infer::get(&head).map(|k| k.mime_type().to_string());
    if let Some(ref mime) = magic_mime {
        if mime.starts_with("image/") {
            return Ok(Detected {
                file_type: FileType::Image,
                magic_mime,
            });
        }
        if let Some(fmt) = archive_format_from_mime(mime) {
            return Ok(Detected {
                file_type: FileType::Archive(fmt),
                magic_mime,
            });
        }
        // Known binary types that aren't text
        if mime.starts_with("video/")
            || mime.starts_with("audio/")
            || mime.starts_with("application/gzip")
            || mime.starts_with("application/x-executable")
        {
            return Ok(Detected {
                file_type: FileType::Binary,
                magic_mime,
            });
        }
    }

    // Stream the file body to check for non-UTF-8 content. Reuses the head
    // buffer as the first chunk so we don't read it twice.
    if !is_utf8_streaming(head, &mut file)? {
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

/// Read into `buf` until full or EOF. Returns the number of bytes read.
/// Unlike `Read::read`, this loops until the buffer is full or the source
/// is exhausted, so partial syscall returns don't truncate the head.
fn read_fill<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// Streaming UTF-8 validation. `head` is the already-read leading chunk;
/// the rest is pulled from `reader` in `SCAN_CHUNK`-sized pieces. The
/// running buffer carries any incomplete trailing UTF-8 sequence (≤3 bytes)
/// across chunk boundaries so multi-byte characters that straddle a chunk
/// boundary are validated correctly.
fn is_utf8_streaming<R: Read>(head: Vec<u8>, reader: &mut R) -> Result<bool> {
    let mut buf = head;
    let mut chunk = vec![0u8; SCAN_CHUNK];
    loop {
        match std::str::from_utf8(&buf) {
            Ok(_) => buf.clear(),
            Err(e) => {
                if e.error_len().is_some() {
                    // A genuine invalid sequence — not text.
                    return Ok(false);
                }
                // Incomplete trailing sequence; drop everything before it
                // and let the next chunk complete it.
                let valid_up_to = e.valid_up_to();
                buf.drain(..valid_up_to);
            }
        }
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            // EOF — anything still buffered is an unfinished sequence.
            return Ok(buf.is_empty());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Match a filename against archive double-extensions (e.g. `.tar.gz`,
/// `.tgz`) and single archive extensions (e.g. `.zip`). Returns `None`
/// for non-archive names. Case-insensitive.
fn archive_format_from_name(name: &str) -> Option<ArchiveFormat> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        return Some(ArchiveFormat::TarGz);
    }
    if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") || lower.ends_with(".tbz") {
        return Some(ArchiveFormat::TarBz2);
    }
    if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        return Some(ArchiveFormat::TarXz);
    }
    if lower.ends_with(".tar.zst") || lower.ends_with(".tzst") {
        return Some(ArchiveFormat::TarZst);
    }
    if lower.ends_with(".tar") {
        return Some(ArchiveFormat::Tar);
    }
    if lower.ends_with(".7z") {
        return Some(ArchiveFormat::SevenZ);
    }
    if lower.ends_with(".zip")
        || lower.ends_with(".jar")
        || lower.ends_with(".war")
        || lower.ends_with(".apk")
    {
        return Some(ArchiveFormat::Zip);
    }
    None
}

/// Map a single file extension to a disk-image format.
fn disk_image_format_from_ext(ext: &str) -> Option<DiskImageFormat> {
    match ext {
        "iso" => Some(DiskImageFormat::Iso),
        _ => None,
    }
}

/// Map an `infer` magic-byte MIME to an archive format. `application/gzip`
/// is intentionally absent: a bare `.gz` isn't necessarily a tarball, so
/// only the extension path treats `*.tar.gz` as `TarGz`.
fn archive_format_from_mime(mime: &str) -> Option<ArchiveFormat> {
    match mime {
        "application/zip" => Some(ArchiveFormat::Zip),
        "application/x-tar" => Some(ArchiveFormat::Tar),
        "application/x-7z-compressed" => Some(ArchiveFormat::SevenZ),
        _ => None,
    }
}

/// Detect the file type from an in-memory byte buffer (for stdin).
/// Uses magic bytes for binary formats, then content sniffing for text.
fn detect_bytes(data: &[u8]) -> Detected {
    let magic_mime = infer::get(data).map(|k| k.mime_type().to_string());
    if let Some(ref mime) = magic_mime {
        if mime == "image/svg+xml" {
            return Detected {
                file_type: FileType::Svg,
                magic_mime,
            };
        }
        if mime.starts_with("image/") {
            return Detected {
                file_type: FileType::Image,
                magic_mime,
            };
        }
        if let Some(fmt) = archive_format_from_mime(mime) {
            return Detected {
                file_type: FileType::Archive(fmt),
                magic_mime,
            };
        }
        if mime.starts_with("video/")
            || mime.starts_with("audio/")
            || mime.starts_with("application/gzip")
            || mime.starts_with("application/x-executable")
        {
            return Detected {
                file_type: FileType::Binary,
                magic_mime,
            };
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

    // Suppress clippy::collapsible_match: rust 1.95 suggests folding the
    // inner `if` into a match guard, but doing so changes fall-through
    // semantics — on guard failure the arm is skipped instead of matched
    // and emptied, so any future arm added below could silently capture it.
    #[allow(clippy::collapsible_match)]
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
                return Detected {
                    file_type: FileType::Svg,
                    magic_mime,
                };
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

    // Plain text — `--language` can still pin a syntax for highlighting.
    Detected {
        file_type: FileType::SourceCode { syntax: None },
        magic_mime,
    }
}
