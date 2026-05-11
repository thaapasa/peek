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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileType {
    /// Source code or text file with optional syntax name
    SourceCode { syntax: Option<String> },
    /// Structured data format
    Structured(StructuredFormat),
    /// Raster image
    Image,
    /// SVG vector image (rasterized for preview, XML source for raw view)
    Svg,
    /// HTML document (rendered text view via html2text, XML source for raw view)
    Html,
    /// EPUB e-book (ZIP container with HTML chapters + OPF metadata)
    Epub,
    /// Comic-archive (one image per page in a ZIP / RAR / 7z / tar
    /// container). Drives the paged-image read mode.
    Comic(ComicFormat),
    /// Word-style document (DOCX = ZIP of XML, RTF = control-word
    /// markup). Drives a styled-text read view; DOCX additionally
    /// exposes the ZIP listing TOC and per-entry extract.
    Document(DocumentFormat),
    /// PDF document. Drives a paged-image render mode + text-extraction
    /// view + embedded-files listing.
    Pdf,
    /// Container archive (zip / tar / compressed tar). Drives the
    /// listing-only TOC viewer — no payload decompression.
    Archive(ArchiveFormat),
    /// Disk image (ISO / DMG / etc). Drives a metadata-only info view —
    /// volume descriptor / trailer parsing, no filesystem walk.
    DiskImage(DiskImageFormat),
    /// Filesystem directory. One-level listing view. Selecting a child
    /// file descends into peek; selecting a child directory re-targets
    /// the current frame (no stack of directories).
    Directory,
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
    /// Unix `ar(1)` archive — used by `.deb` packages (Debian binary
    /// package layout: `debian-binary`, `control.tar.*`, `data.tar.*`).
    Ar,
    /// Bare gzip stream (`.gz`). Treated as a one-entry archive so
    /// the listing / descend / extract pipeline lights up for it.
    Gz,
    /// Bare bzip2 stream (`.bz2`).
    Bz2,
    /// Bare xz / LZMA2 stream (`.xz`).
    Xz,
    /// Bare zstd stream (`.zst`).
    Zst,
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
            Self::Ar => "ar archive",
            Self::Gz => "gzip stream",
            Self::Bz2 => "bzip2 stream",
            Self::Xz => "xz stream",
            Self::Zst => "zstd stream",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComicFormat {
    /// Comic Book ZIP (most common comic-archive form in the wild).
    Cbz,
}

impl ComicFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cbz => "Comic Book ZIP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormat {
    /// Office Open XML word-processing document. ZIP container with
    /// `word/document.xml` body + `docProps/*.xml` metadata.
    Docx,
    /// Rich Text Format. Control-word markup; single file, not a
    /// container.
    Rtf,
}

impl DocumentFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Docx => "DOCX document",
            Self::Rtf => "RTF document",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskImageFormat {
    Iso,
    Dmg,
    /// Generic raw disk image (`.img` / `.bin` / `.dd`) that doesn't
    /// match a recognised filesystem header. Listing isn't supported
    /// — the info section parses the partition table when one is
    /// present, otherwise falls back to "raw image".
    Raw,
}

impl DiskImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Iso => "ISO 9660 image",
            Self::Dmg => "Apple Disk Image (UDIF)",
            Self::Raw => "Raw disk image",
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
    detect_with(source, false)
}

/// Re-detect ignoring the source's path / entry name. Used as a
/// fallback retry when rendering fails — if the file's extension lied
/// about the content, magic-byte detection on the body still resolves
/// the real type.
pub fn detect_ignore_name(source: &InputSource) -> Result<Detected> {
    detect_with(source, true)
}

fn detect_with(source: &InputSource, ignore_name: bool) -> Result<Detected> {
    match source {
        InputSource::File(path) => detect_file(path, ignore_name),
        InputSource::Memory { bytes, name } => Ok(detect_bytes_named(
            bytes,
            if ignore_name {
                None
            } else {
                Some(name.as_str())
            },
        )),
        InputSource::FileRange { name, .. } => {
            let buf = source.read_bytes()?;
            Ok(detect_bytes_named(
                &buf,
                if ignore_name {
                    None
                } else {
                    Some(name.as_str())
                },
            ))
        }
    }
}

fn detect_file(path: &Path, ignore_name: bool) -> Result<Detected> {
    if !path.exists() {
        bail!("file not found: {}", path.display());
    }

    // Directories get their own one-level listing viewer; everything
    // below assumes a regular file we can read bytes from.
    if path.is_dir() {
        return Ok(Detected {
            file_type: FileType::Directory,
            magic_mime: None,
        });
    }

    // Read just the head for magic-byte detection — `infer` only inspects
    // the first few hundred bytes, so we never need the whole file. Done
    // up front (before extension routing) so the magic-byte MIME flows
    // into `Detected.magic_mime` even when the extension is what picks
    // the viewer. Downstream info section uses both to flag
    // extension/MIME mismatches.
    let mut file = fs::File::open(path)?;
    let mut head = vec![0u8; HEAD_BYTES];
    let n = read_fill(&mut file, &mut head)?;
    head.truncate(n);
    let head_magic = head_magic_mime(&head);

    if !ignore_name {
        // Archive double-extensions (.tar.gz, .tgz, etc.) check the full file
        // name, so they win over the single-extension fallback below for files
        // like `archive.tar.gz` where `extension()` would only see `.gz`.
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && let Some(fmt) = archive_format_from_name(name)
        {
            return Ok(Detected {
                file_type: FileType::Archive(fmt),
                magic_mime: head_magic,
            });
        }

        // Comic-archive extensions win over the magic-byte ZIP detection
        // below so a `.cbz` doesn't fall through to FileType::Archive(Zip).
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && let Some(fmt) = comic_format_from_ext(&ext.to_lowercase())
        {
            return Ok(Detected {
                file_type: FileType::Comic(fmt),
                magic_mime: head_magic,
            });
        }

        // Disk-image extensions resolve before the structured/text fallback so
        // the single-extension match below doesn't ever see them.
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && let Some(fmt) = disk_image_format_from_ext(&ext.to_lowercase())
        {
            // `.img` is ambiguous: many distributions ship ISO data under
            // a `.img` extension, while others use it for raw block-level
            // dumps. Probe the ISO 9660 PVD signature first; treat as ISO
            // when it matches, otherwise as a generic raw image.
            let resolved = if matches!(fmt, DiskImageFormat::Raw) {
                probe_iso_or_raw(path).unwrap_or(DiskImageFormat::Raw)
            } else {
                fmt
            };
            return Ok(Detected {
                file_type: FileType::DiskImage(resolved),
                magic_mime: head_magic,
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
                "html" | "htm" | "xhtml" => Some(FileType::Html),
                "epub" => Some(FileType::Epub),
                "docx" => Some(FileType::Document(DocumentFormat::Docx)),
                "rtf" => Some(FileType::Document(DocumentFormat::Rtf)),
                "pdf" => Some(FileType::Pdf),
                "xml" | "plist" => Some(FileType::Structured(StructuredFormat::Xml)),
                _ => None,
            };
            if let Some(file_type) = file_type {
                return Ok(Detected {
                    file_type,
                    magic_mime: head_magic,
                });
            }
        }
    }

    if head.len() >= AR_MAGIC.len() && &head[..AR_MAGIC.len()] == AR_MAGIC {
        return Ok(Detected {
            file_type: FileType::Archive(ArchiveFormat::Ar),
            magic_mime: Some("application/x-archive".to_string()),
        });
    }

    // RTF starts with `{\rtf1`. `infer` doesn't recognise it; without
    // this probe stdin-piped RTF would fall through to text/source-code
    // and lose the styled rendering path.
    if head.starts_with(RTF_MAGIC) {
        return Ok(Detected {
            file_type: FileType::Document(DocumentFormat::Rtf),
            magic_mime: Some("application/rtf".to_string()),
        });
    }

    if head.starts_with(PDF_MAGIC) {
        return Ok(Detected {
            file_type: FileType::Pdf,
            magic_mime: Some("application/pdf".to_string()),
        });
    }

    let magic_mime = head_magic;
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

    // Content-sniff the head (cheap, ASCII-pattern based) BEFORE
    // streaming the whole body for UTF-8 validation — sniffing only
    // needs the head bytes, and the result fills in `magic_mime` for
    // text formats `infer` doesn't classify (SVG / HTML / XML / JSON /
    // YAML). Compute now so the head buffer can move into the streaming
    // UTF-8 check below.
    let sniffed = std::str::from_utf8(&head).ok().and_then(sniff_text_content);

    // Stream the file body to check for non-UTF-8 content. Reuses the head
    // buffer as the first chunk so we don't read it twice.
    if !is_utf8_streaming(head, &mut file)? {
        return Ok(Detected {
            file_type: FileType::Binary,
            magic_mime,
        });
    }

    if let Some((file_type, content_mime)) = sniffed {
        return Ok(Detected {
            file_type,
            magic_mime: magic_mime.or_else(|| Some(content_mime.to_string())),
        });
    }

    // It's a text file — use extension as syntax hint (unless we're
    // ignoring the name on a fallback retry).
    let syntax = if ignore_name {
        None
    } else {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
    };

    Ok(Detected {
        file_type: FileType::SourceCode { syntax },
        magic_mime,
    })
}

/// Magic-byte MIME for a file head. Combines the explicit AR / RTF /
/// PDF prefix probes (which `infer` doesn't classify) with
/// `infer::get`. Returned MIME flows into `Detected.magic_mime` so the
/// info section can flag extension/MIME mismatches even when the
/// extension was the thing that picked the viewer.
fn head_magic_mime(head: &[u8]) -> Option<String> {
    if head.len() >= AR_MAGIC.len() && &head[..AR_MAGIC.len()] == AR_MAGIC {
        return Some("application/x-archive".to_string());
    }
    if head.starts_with(RTF_MAGIC) {
        return Some("application/rtf".to_string());
    }
    if head.starts_with(PDF_MAGIC) {
        return Some("application/pdf".to_string());
    }
    infer::get(head).map(|k| k.mime_type().to_string())
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
    if lower.ends_with(".deb") || lower.ends_with(".ar") || lower.ends_with(".a") {
        return Some(ArchiveFormat::Ar);
    }
    // Bare single-stream codec extensions. Order matters: the
    // tar.* variants above already matched and returned before any
    // file with a `.tar.gz` etc. name reaches this point.
    if lower.ends_with(".gz") {
        return Some(ArchiveFormat::Gz);
    }
    if lower.ends_with(".bz2") {
        return Some(ArchiveFormat::Bz2);
    }
    if lower.ends_with(".xz") {
        return Some(ArchiveFormat::Xz);
    }
    if lower.ends_with(".zst") {
        return Some(ArchiveFormat::Zst);
    }
    None
}

/// Map a single file extension to a comic-archive format.
fn comic_format_from_ext(ext: &str) -> Option<ComicFormat> {
    match ext {
        "cbz" => Some(ComicFormat::Cbz),
        _ => None,
    }
}

/// Map a single file extension to a disk-image format. `.img` /
/// `.bin` / `.dd` map to `Raw` provisionally; the caller probes for
/// an ISO 9660 PVD before committing to that classification.
fn disk_image_format_from_ext(ext: &str) -> Option<DiskImageFormat> {
    match ext {
        "iso" => Some(DiskImageFormat::Iso),
        "dmg" => Some(DiskImageFormat::Dmg),
        "img" | "bin" | "dd" => Some(DiskImageFormat::Raw),
        _ => None,
    }
}

/// Read 6 bytes at the ISO 9660 PVD location (offset 32768 + 0..=5)
/// and check for the `\x01CD001` signature. Returns
/// `Some(DiskImageFormat::Iso)` on match, `None` otherwise (caller
/// falls back to `Raw`).
fn probe_iso_or_raw(path: &Path) -> Option<DiskImageFormat> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(32768)).ok()?;
    let mut buf = [0u8; 6];
    file.read_exact(&mut buf).ok()?;
    if buf[0] == 1 && &buf[1..6] == b"CD001" {
        Some(DiskImageFormat::Iso)
    } else {
        None
    }
}

/// Map an `infer` magic-byte MIME to an archive format. The compressed
/// single-stream variants (gzip / bzip2 / xz / zstd) treat the file as
/// a one-entry archive, so the listing / extract pipeline gives the
/// user a path into the decompressed content.
fn archive_format_from_mime(mime: &str) -> Option<ArchiveFormat> {
    match mime {
        "application/zip" => Some(ArchiveFormat::Zip),
        "application/x-tar" => Some(ArchiveFormat::Tar),
        "application/x-7z-compressed" => Some(ArchiveFormat::SevenZ),
        "application/gzip" | "application/x-gzip" => Some(ArchiveFormat::Gz),
        "application/x-bzip2" | "application/x-bzip" => Some(ArchiveFormat::Bz2),
        "application/x-xz" => Some(ArchiveFormat::Xz),
        "application/zstd" | "application/x-zstd" => Some(ArchiveFormat::Zst),
        _ => None,
    }
}

/// 8-byte ar archive magic — `!<arch>\n`. `infer` doesn't recognise
/// ar; without an explicit check, stdin-piped `.deb` files would
/// classify as binary.
const AR_MAGIC: &[u8; 8] = b"!<arch>\n";

/// RTF (Rich Text Format) signature. Every conforming RTF starts with
/// `{\rtf1`; `infer` doesn't classify RTF, so the explicit prefix
/// match is what routes stdin-piped RTF away from plain text.
const RTF_MAGIC: &[u8] = b"{\\rtf1";

/// PDF signature. Every conforming PDF starts with `%PDF-1.x` (v1.x)
/// or `%PDF-2.0` (v2.0). `infer` recognises PDFs but only for the
/// canonical path — the explicit prefix check keeps stdin-piped PDFs
/// reliable across infer versions.
const PDF_MAGIC: &[u8] = b"%PDF-";

/// Inspect a UTF-8 text buffer for a recognisable structured /
/// markup format. Returns the detected `FileType` plus a canonical
/// MIME so the caller can populate `Detected.magic_mime` when
/// `infer` didn't classify the bytes (it never identifies plain
/// text/XML formats). Used by both file and byte detection paths so
/// the rules stay in one place.
fn sniff_text_content(text: &str) -> Option<(FileType, &'static str)> {
    let trimmed = text.trim_start();
    let first = trimmed.as_bytes().first().copied();
    #[allow(clippy::collapsible_match)]
    match first {
        Some(b'{') | Some(b'[') => {
            if serde_json::from_str::<serde_json::Value>(text).is_ok() {
                return Some((
                    FileType::Structured(StructuredFormat::Json),
                    "application/json",
                ));
            }
        }
        Some(b'<') => {
            if trimmed.contains("<svg") {
                return Some((FileType::Svg, "image/svg+xml"));
            }
            let head_lower = trimmed[..trimmed.len().min(512)].to_ascii_lowercase();
            if head_lower.starts_with("<!doctype html") || head_lower.contains("<html") {
                return Some((FileType::Html, "text/html"));
            }
            return Some((
                FileType::Structured(StructuredFormat::Xml),
                "application/xml",
            ));
        }
        _ => {}
    }
    if trimmed.starts_with("---\n")
        || trimmed.starts_with("---\r\n")
        || trimmed == "---"
        || trimmed.starts_with("%YAML")
    {
        return Some((
            FileType::Structured(StructuredFormat::Yaml),
            "application/yaml",
        ));
    }
    None
}

/// Detect the file type from an in-memory byte buffer (for stdin).
/// Uses magic bytes for binary formats, then content sniffing for text.
fn detect_bytes(data: &[u8]) -> Detected {
    if data.len() >= AR_MAGIC.len() && &data[..AR_MAGIC.len()] == AR_MAGIC {
        return Detected {
            file_type: FileType::Archive(ArchiveFormat::Ar),
            magic_mime: Some("application/x-archive".to_string()),
        };
    }
    if data.starts_with(RTF_MAGIC) {
        return Detected {
            file_type: FileType::Document(DocumentFormat::Rtf),
            magic_mime: Some("application/rtf".to_string()),
        };
    }
    if data.starts_with(PDF_MAGIC) {
        return Detected {
            file_type: FileType::Pdf,
            magic_mime: Some("application/pdf".to_string()),
        };
    }
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

    if let Some((file_type, content_mime)) = sniff_text_content(text) {
        return Detected {
            file_type,
            magic_mime: magic_mime.or_else(|| Some(content_mime.to_string())),
        };
    }

    // Plain text — `--language` can still pin a syntax for highlighting.
    Detected {
        file_type: FileType::SourceCode { syntax: None },
        magic_mime,
    }
}

/// Detect from a byte buffer with an optional source name. The name is
/// consulted first for extension-based classification (so a file
/// extracted from an archive into memory still routes by `.json` /
/// `.svg` / etc. just like a real path would), then for a syntect
/// syntax hint if content sniffing only resolves to plain SourceCode.
///
/// Used by `detect()` for `Memory` and `FileRange` sources so recursive
/// peek into a container (EPUB / archive / ISO) doesn't lose the entry
/// name's classification on its way back through the pipeline.
fn detect_bytes_named(data: &[u8], name: Option<&str>) -> Detected {
    if let Some(name) = name
        && let Some(file_type) = classify_by_name(name)
    {
        return Detected {
            file_type,
            magic_mime: head_magic_mime(data),
        };
    }
    let mut detected = detect_bytes(data);
    if let FileType::SourceCode { syntax: None } = &detected.file_type
        && let Some(ext) = name.and_then(extension_lower)
    {
        detected.file_type = FileType::SourceCode { syntax: Some(ext) };
    }
    detected
}

/// Mirror of `detect_file`'s extension routing for name-only sources.
/// Covers archive double-extensions, disk-image extensions, and the
/// structured / SVG / HTML / EPUB family. Returns `None` when no
/// extension matches and the caller should fall back to content
/// sniffing.
fn classify_by_name(name: &str) -> Option<FileType> {
    if let Some(fmt) = archive_format_from_name(name) {
        return Some(FileType::Archive(fmt));
    }
    let ext = extension_lower(name)?;
    if let Some(fmt) = comic_format_from_ext(&ext) {
        return Some(FileType::Comic(fmt));
    }
    if let Some(fmt) = disk_image_format_from_ext(&ext) {
        return Some(FileType::DiskImage(fmt));
    }
    Some(match ext.as_str() {
        "json" | "geojson" => FileType::Structured(StructuredFormat::Json),
        "jsonc" => FileType::Structured(StructuredFormat::Jsonc),
        "json5" => FileType::Structured(StructuredFormat::Json5),
        "jsonl" | "ndjson" => FileType::Structured(StructuredFormat::Jsonl),
        "yaml" | "yml" => FileType::Structured(StructuredFormat::Yaml),
        "toml" => FileType::Structured(StructuredFormat::Toml),
        "svg" => FileType::Svg,
        "html" | "htm" | "xhtml" => FileType::Html,
        "epub" => FileType::Epub,
        "docx" => FileType::Document(DocumentFormat::Docx),
        "rtf" => FileType::Document(DocumentFormat::Rtf),
        "pdf" => FileType::Pdf,
        "xml" | "plist" => FileType::Structured(StructuredFormat::Xml),
        _ => return None,
    })
}

/// Lowercased extension of a file name, or `None` for hidden files
/// (`.foo`) and names without an extension.
fn extension_lower(name: &str) -> Option<String> {
    let pos = name.rfind('.')?;
    if pos == 0 || pos == name.len() - 1 {
        return None;
    }
    Some(name[pos + 1..].to_ascii_lowercase())
}
