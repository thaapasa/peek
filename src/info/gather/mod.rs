//! Per-source dispatch for file-info gathering.
//!
//! `gather()` is the only public entry point; the type-specific gathering
//! lives in submodules grouped by general file type:
//!
//! * `image`     — raster images (also pulls in `exif`, `xmp`, `animation`)
//! * `text`      — source code and other UTF-8 / UTF-16 text content
//! * `structured` — JSON / YAML / TOML / XML
//! * `svg`       — SVG files (image + text dual nature)
//! * `binary`    — fallback labelling for unrecognised binary content
//!
//! All submodules return [`FileExtras`] payloads. This module only chooses
//! which one to call.

use std::fs;
use std::path::Path;

use anyhow::Result;

use super::{FileExtras, FileInfo, format_permissions_from_meta};
use crate::input::InputSource;
use crate::input::detect::{ComicFormat, Detected, DocumentFormat, FileType};
use crate::input::mime;

#[cfg(test)]
mod tests;

use crate::types::text::info_gather::gather_text_stats;

/// Cap on bytes parsed for the per-language sidecar stats (markdown / SQL).
/// Above this we keep the streaming text stats and skip the language-specific
/// pass — so multi-GB SQL dumps stay openable without burning RAM on a parse
/// that would just be noise anyway.
const LANG_STATS_BYTE_LIMIT: u64 = 64 * 1024 * 1024;

fn is_markdown_syntax(syntax: Option<&str>) -> bool {
    matches!(
        syntax,
        Some("md" | "markdown" | "mdown" | "mkd" | "mkdn" | "mdwn")
    )
}

fn is_sql_syntax(syntax: Option<&str>) -> bool {
    matches!(syntax, Some("sql" | "ddl" | "dml" | "psql" | "pgsql"))
}

fn syntax_of(file_type: &FileType) -> Option<&str> {
    match file_type {
        FileType::SourceCode { syntax } => syntax.as_deref(),
        _ => None,
    }
}

/// Try the language-specific sidecar parse for a SourceCode file. Returns
/// `None` if `file_type` isn't a recognised flavour, the source is too big,
/// or the read fails.
fn gather_code_extras(source: &InputSource, file_type: &FileType) -> Option<FileExtras> {
    let syntax = syntax_of(file_type)?;
    let is_md = is_markdown_syntax(Some(syntax));
    let is_sql = is_sql_syntax(Some(syntax));
    if !is_md && !is_sql {
        return None;
    }

    let bs = source.open_byte_source().ok()?;
    if bs.len() > LANG_STATS_BYTE_LIMIT {
        return None;
    }

    let text_stats = gather_text_stats(source)?;
    let text = source.read_text().ok()?;

    if is_md {
        let stats = crate::types::markdown::info_gather::gather(&text);
        Some(FileExtras::Markdown {
            text: text_stats,
            stats,
        })
    } else {
        let stats = crate::types::sql::info_gather::gather(&text);
        Some(FileExtras::Sql {
            text: text_stats,
            stats,
        })
    }
}

/// Gather metadata for the given input source and detection result.
///
/// `detected.magic_mime` is reused (no re-read of the file) to build the
/// MIME list and to detect extension/content mismatches.
pub fn gather(source: &InputSource, detected: &Detected) -> Result<FileInfo> {
    match source {
        InputSource::File(path) => gather_file(path, detected),
        // Memory + FileRange share the "no filesystem metadata" path —
        // they don't have an mtime, owner, or stat() to draw from. Size
        // comes from the byte source, name from the source's display name.
        _ => Ok(gather_virtual(source, detected)),
    }
}

fn gather_file(path: &Path, detected: &Detected) -> Result<FileInfo> {
    let meta = fs::metadata(path)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let display_path = path.to_string_lossy().into_owned();

    let mimes = mime::mimes_for_path(
        &detected.file_type,
        Some(path),
        detected.magic_mime.as_deref(),
    );
    let warnings = collect_warnings(Some(path), detected);

    let permissions = format_permissions_from_meta(&meta);
    let extras = gather_extras(path, &detected.file_type, detected.magic_mime.as_deref());

    Ok(FileInfo {
        file_name,
        path: display_path,
        size_bytes: meta.len(),
        mimes,
        warnings,
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        permissions,
        extras,
    })
}

fn gather_virtual(source: &InputSource, detected: &Detected) -> FileInfo {
    let mimes = mime::mimes_for_path(&detected.file_type, None, detected.magic_mime.as_deref());
    let warnings = collect_warnings(None, detected);
    let extras =
        gather_extras_in_memory(source, &detected.file_type, detected.magic_mime.as_deref());
    let size = source.open_byte_source().map(|bs| bs.len()).unwrap_or(0);
    let display = source.name().to_string();

    FileInfo {
        file_name: display.clone(),
        path: display,
        size_bytes: size,
        mimes,
        warnings,
        modified: None,
        created: None,
        permissions: None,
        extras,
    }
}

/// Build the warnings list. Currently: extension-mismatch only (file path).
fn collect_warnings(path: Option<&Path>, detected: &Detected) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(p) = path
        && let Some(w) = mime::extension_mismatch(p, detected.magic_mime.as_deref())
    {
        warnings.push(w);
    }
    warnings
}

fn gather_extras_in_memory(
    source: &InputSource,
    file_type: &FileType,
    magic_mime: Option<&str>,
) -> FileExtras {
    match file_type {
        FileType::SourceCode { .. } => {
            if let Some(extras) = gather_code_extras(source, file_type) {
                return extras;
            }
            match gather_text_stats(source) {
                Some(stats) => FileExtras::Text(stats),
                None => crate::types::binary::info::gather_extras(magic_mime),
            }
        }
        FileType::Svg => match (gather_text_stats(source), source.read_bytes()) {
            (Some(stats), Ok(bytes)) => {
                crate::types::svg::info_gather::gather_extras(stats, &bytes)
            }
            _ => crate::types::binary::info::gather_extras(magic_mime),
        },
        FileType::Structured(fmt) => match source.read_bytes() {
            Ok(bytes) => crate::types::structured::info::gather_extras(*fmt, &bytes),
            Err(_) => FileExtras::Structured {
                format_name: crate::types::structured::info::format_name(*fmt),
                stats: None,
            },
        },
        FileType::Html => match source.read_bytes() {
            Ok(bytes) => crate::types::structured::info::gather_extras(
                crate::input::detect::StructuredFormat::Xml,
                &bytes,
            ),
            Err(_) => FileExtras::Structured {
                format_name: "HTML",
                stats: None,
            },
        },
        FileType::Epub => crate::types::ebook::epub::info_gather::gather_extras(source),
        FileType::Comic(fmt @ ComicFormat::Cbz) => {
            crate::types::comic::cbz::info_gather::gather_extras(source, *fmt)
        }
        FileType::Document(DocumentFormat::Docx) => {
            crate::types::document::docx::info_gather::gather_extras(source)
        }
        FileType::Document(DocumentFormat::Rtf) => {
            crate::types::document::rtf::info_gather::gather_extras(source)
        }
        FileType::Pdf => crate::types::pdf::info_gather::gather_extras(source),
        FileType::Image => crate::types::image::info_gather::gather_extras(source, magic_mime),
        FileType::Archive(fmt) => crate::types::archive::info::gather_extras(source, *fmt),
        FileType::DiskImage(fmt) => {
            crate::types::disk_image::info_gather::gather_extras(source, *fmt)
        }
        // Directory only ever appears via a real `File` source; the
        // virtual-source path can't construct one.
        FileType::Directory => crate::types::binary::info::gather_extras(magic_mime),
        FileType::Binary => crate::types::binary::info::gather_extras(magic_mime),
    }
}

fn gather_extras(path: &Path, file_type: &FileType, magic_mime: Option<&str>) -> FileExtras {
    match file_type {
        FileType::Image => crate::types::image::info_gather::gather_extras(
            &InputSource::File(path.to_path_buf()),
            magic_mime,
        ),
        FileType::SourceCode { .. } => {
            let source = InputSource::File(path.to_path_buf());
            if let Some(extras) = gather_code_extras(&source, file_type) {
                return extras;
            }
            match gather_text_stats(&source) {
                Some(stats) => FileExtras::Text(stats),
                None => crate::types::binary::info::gather_extras(magic_mime),
            }
        }
        FileType::Svg => {
            let source = InputSource::File(path.to_path_buf());
            match (gather_text_stats(&source), source.read_bytes()) {
                (Some(stats), Ok(bytes)) => {
                    crate::types::svg::info_gather::gather_extras(stats, &bytes)
                }
                _ => crate::types::binary::info::gather_extras(magic_mime),
            }
        }
        FileType::Structured(fmt) => match fs::read(path) {
            Ok(bytes) => crate::types::structured::info::gather_extras(*fmt, &bytes),
            Err(_) => FileExtras::Structured {
                format_name: crate::types::structured::info::format_name(*fmt),
                stats: None,
            },
        },
        FileType::Html => match fs::read(path) {
            Ok(bytes) => crate::types::structured::info::gather_extras(
                crate::input::detect::StructuredFormat::Xml,
                &bytes,
            ),
            Err(_) => FileExtras::Structured {
                format_name: "HTML",
                stats: None,
            },
        },
        FileType::Archive(fmt) => {
            crate::types::archive::info::gather_extras(&InputSource::File(path.to_path_buf()), *fmt)
        }
        FileType::Epub => crate::types::ebook::epub::info_gather::gather_extras(
            &InputSource::File(path.to_path_buf()),
        ),
        FileType::Comic(fmt @ ComicFormat::Cbz) => {
            crate::types::comic::cbz::info_gather::gather_extras(
                &InputSource::File(path.to_path_buf()),
                *fmt,
            )
        }
        FileType::Document(DocumentFormat::Docx) => {
            crate::types::document::docx::info_gather::gather_extras(&InputSource::File(
                path.to_path_buf(),
            ))
        }
        FileType::Document(DocumentFormat::Rtf) => {
            crate::types::document::rtf::info_gather::gather_extras(&InputSource::File(
                path.to_path_buf(),
            ))
        }
        FileType::Pdf => {
            crate::types::pdf::info_gather::gather_extras(&InputSource::File(path.to_path_buf()))
        }
        FileType::DiskImage(fmt) => crate::types::disk_image::info_gather::gather_extras(
            &InputSource::File(path.to_path_buf()),
            *fmt,
        ),
        FileType::Directory => crate::types::directory::info::gather_extras(path),
        FileType::Binary => crate::types::binary::info::gather_extras(magic_mime),
    }
}
