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

mod animation;
mod binary;
mod exif;
mod image;
mod structured;
mod svg;
mod text;
mod xmp;

#[cfg(test)]
mod tests;

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
        FileType::SourceCode { .. } => match text::gather_text_stats(&stdin_source) {
            Some(stats) => FileExtras::Text(stats),
            None => binary::binary_extras(magic_mime),
        },
        FileType::Svg => match text::gather_text_stats(&stdin_source) {
            Some(stats) => svg::svg_extras(stats, data),
            None => binary::binary_extras(magic_mime),
        },
        FileType::Structured(fmt) => structured::structured_extras(*fmt, data),
        FileType::Image => image::gather_image_extras_from_bytes(data, magic_mime),
        FileType::Binary => binary::binary_extras(magic_mime),
    }
}

fn gather_extras(path: &Path, file_type: &FileType, magic_mime: Option<&str>) -> FileExtras {
    match file_type {
        FileType::Image => image::gather_image_extras(path, magic_mime),
        FileType::SourceCode { .. } => {
            match text::gather_text_stats(&InputSource::File(path.to_path_buf())) {
                Some(stats) => FileExtras::Text(stats),
                None => binary::binary_extras(magic_mime),
            }
        }
        FileType::Svg => {
            let source = InputSource::File(path.to_path_buf());
            match (text::gather_text_stats(&source), source.read_bytes()) {
                (Some(stats), Ok(bytes)) => svg::svg_extras(stats, &bytes),
                _ => binary::binary_extras(magic_mime),
            }
        }
        FileType::Structured(fmt) => match fs::read(path) {
            Ok(bytes) => structured::structured_extras(*fmt, &bytes),
            Err(_) => FileExtras::Structured {
                format_name: structured::format_name(*fmt),
                stats: None,
            },
        },
        FileType::Binary => binary::binary_extras(magic_mime),
    }
}
