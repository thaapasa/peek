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
use std::sync::Arc;

use anyhow::Result;

use super::{FileExtras, FileInfo, format_permissions_from_meta};
use crate::input::InputSource;
use crate::input::detect::{Detected, FileType};
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
        InputSource::Stdin { data } => Ok(gather_stdin(data, detected)),
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

fn gather_stdin(data: &Arc<[u8]>, detected: &Detected) -> FileInfo {
    let mimes = mime::mimes_for_path(&detected.file_type, None, detected.magic_mime.as_deref());
    let warnings = collect_warnings(None, detected);
    let extras = gather_extras_stdin(data, &detected.file_type, detected.magic_mime.as_deref());

    FileInfo {
        file_name: "<stdin>".to_string(),
        path: "<stdin>".to_string(),
        size_bytes: data.len() as u64,
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

fn gather_extras_stdin(
    data: &Arc<[u8]>,
    file_type: &FileType,
    magic_mime: Option<&str>,
) -> FileExtras {
    let stdin_source = InputSource::Stdin {
        data: Arc::clone(data),
    };
    match file_type {
        FileType::SourceCode { .. } => {
            if let Some(extras) = gather_code_extras(&stdin_source, file_type) {
                return extras;
            }
            match gather_text_stats(&stdin_source) {
                Some(stats) => FileExtras::Text(stats),
                None => crate::types::binary::info::gather_extras(magic_mime),
            }
        }
        FileType::Svg => match gather_text_stats(&stdin_source) {
            Some(stats) => crate::types::svg::info_gather::gather_extras(stats, data),
            None => crate::types::binary::info::gather_extras(magic_mime),
        },
        FileType::Structured(fmt) => crate::types::structured::info::gather_extras(*fmt, data),
        FileType::Image => {
            crate::types::image::info_gather::gather_extras(&stdin_source, magic_mime)
        }
        FileType::Archive(fmt) => crate::types::archive::info::gather_extras(&stdin_source, *fmt),
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
        FileType::Archive(fmt) => {
            crate::types::archive::info::gather_extras(&InputSource::File(path.to_path_buf()), *fmt)
        }
        FileType::Binary => crate::types::binary::info::gather_extras(magic_mime),
    }
}
