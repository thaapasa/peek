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
//! All submodules return [`Extras`] payloads (a boxed `dyn InfoExtras`).
//! This module only chooses which one to call.

use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::info::{CompressionInfo, Extras, FileInfo, format_permissions_from_meta};
use crate::input::InputSource;
use crate::input::detect::{
    CertFormat, ComicFormat, CsvFormat, DecompressionContext, Detected, DocumentFormat,
    EbookFormat, FileType, FontFormat,
};
use crate::input::mime;

#[cfg(test)]
mod tests;

use crate::types::text::info_gather::gather_text_stats;

/// Cap on bytes parsed for the per-language sidecar stats (markdown / SQL).
/// Above this we keep the streaming text stats and skip the language-specific
/// pass — so multi-GB SQL dumps stay openable without burning RAM on a parse
/// that would just be noise anyway.
const LANG_STATS_BYTE_LIMIT: u64 = 64 * 1024 * 1024;

fn is_sql_syntax(syntax: Option<&str>) -> bool {
    matches!(syntax, Some("sql" | "ddl" | "dml" | "psql" | "pgsql"))
}

fn is_css_syntax(syntax: Option<&str>) -> bool {
    // Plain CSS only — `.scss` / `.less` are different grammars and keep
    // the generic text-stats fallback.
    matches!(syntax, Some("css"))
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
fn gather_code_extras(source: &InputSource, file_type: &FileType) -> Option<Extras> {
    let syntax = syntax_of(file_type)?;
    let is_sql = is_sql_syntax(Some(syntax));
    let is_css = is_css_syntax(Some(syntax));
    if !is_sql && !is_css {
        return None;
    }

    let bs = source.open_byte_source().ok()?;
    if bs.len() > LANG_STATS_BYTE_LIMIT {
        return None;
    }

    let text_stats = gather_text_stats(source)?;
    let text = source.read_text().ok()?;

    if is_sql {
        let stats = crate::types::sql::info_gather::gather(&text);
        Some(Box::new(crate::types::sql::info::SqlInfo {
            text: text_stats,
            stats,
        }))
    } else {
        let stats = crate::types::css::info_gather::gather(&text);
        Some(Box::new(crate::types::css::info::CssInfo {
            text: text_stats,
            stats,
        }))
    }
}

/// Markdown sidecar parse. Reads the file once, runs both the generic
/// text stats and the markdown-specific scanner. Capped at
/// `LANG_STATS_BYTE_LIMIT` — over the cap the binary fallback applies.
fn gather_markdown_extras(source: &InputSource) -> Option<Extras> {
    let bs = source.open_byte_source().ok()?;
    if bs.len() > LANG_STATS_BYTE_LIMIT {
        return None;
    }
    let text_stats = gather_text_stats(source)?;
    let text = source.read_text().ok()?;
    let stats = crate::types::markdown::info_gather::gather(&text);
    Some(Box::new(crate::types::markdown::info::MarkdownInfo {
        text: text_stats,
        stats,
    }))
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
    let warnings = collect_warnings(&file_name, detected);

    let permissions = format_permissions_from_meta(&meta);
    let source = InputSource::File(path.to_path_buf());
    let extras = gather_extras(&source, &detected.file_type, detected.magic_mime.as_deref());
    let compression = build_compression_info(detected, meta.len());

    Ok(FileInfo {
        file_name,
        path: display_path,
        size_bytes: meta.len(),
        mimes,
        warnings,
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        permissions,
        compression,
        extras,
    })
}

fn gather_virtual(source: &InputSource, detected: &Detected) -> FileInfo {
    let mimes = mime::mimes_for_path(&detected.file_type, None, detected.magic_mime.as_deref());
    let warnings = collect_warnings(source.name(), detected);
    let extras = gather_extras(source, &detected.file_type, detected.magic_mime.as_deref());
    let size = source.open_byte_source().map(|bs| bs.len()).unwrap_or(0);
    let compression = build_compression_info(detected, size);

    // When the source is the inner content of a transparently-
    // decompressed wrapper, show the outer (compressed) name + size
    // in the Name/Path/Size rows — that's the file the user typed.
    // The Compression row separately surfaces the decompressed size.
    let (display, file_size) = match &compression {
        Some(c) if c.error.is_none() => (c.outer_name.clone(), c.compressed_size),
        _ => (source.name().to_string(), size),
    };

    FileInfo {
        file_name: display.clone(),
        path: display,
        size_bytes: file_size,
        mimes,
        warnings,
        modified: None,
        created: None,
        permissions: None,
        compression,
        extras,
    }
}

/// Build the Compression info row from `Detected.decompressed_from`,
/// when present. `decompressed_size` is the rendered (inner) source's
/// size — for the success path that's the in-memory decompressed bytes;
/// for the failure path it's the raw compressed bytes (same as
/// `compressed_size`), since the viewer is showing those directly.
fn build_compression_info(detected: &Detected, decompressed_size: u64) -> Option<CompressionInfo> {
    let ctx: &DecompressionContext = detected.decompressed_from.as_ref()?;
    Some(CompressionInfo {
        codec_label: ctx.codec.codec_label(),
        compressed_size: ctx.compressed_size,
        decompressed_size,
        outer_name: ctx.outer_name.clone(),
        error: ctx.error.clone(),
    })
}

/// Build the warnings list. Sources today: extension-mismatch +
/// decompression failure on a bare-codec source.
fn collect_warnings(name: &str, detected: &Detected) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(ext) = mime::extension_from_name(name)
        && let Some(w) =
            mime::extension_mismatch(&ext, detected.magic_mime.as_deref(), &detected.file_type)
    {
        warnings.push(w);
    }
    if let Some(ctx) = &detected.decompressed_from
        && let Some(err) = &ctx.error
    {
        warnings.push(format!(
            "decompression failed ({}): {err}",
            ctx.codec.codec_label()
        ));
    }
    warnings
}

/// Gather the per-type [`Extras`] payload for an already-detected file.
///
/// Every file type goes through `&InputSource` — `InputSource::File` reads
/// on demand, so a real file and a virtual (Memory / FileRange) source share
/// the same arms with no duplicated dispatch table. `Directory` is the lone
/// arm needing a real path, and it only ever arrives via a `File` source.
fn gather_extras(source: &InputSource, file_type: &FileType, magic_mime: Option<&str>) -> Extras {
    match file_type {
        FileType::SourceCode { .. } => {
            if let Some(extras) = gather_code_extras(source, file_type) {
                return extras;
            }
            match gather_text_stats(source) {
                Some(stats) => Box::new(stats),
                None => crate::types::binary::info::gather_extras(magic_mime),
            }
        }
        FileType::Markdown => match gather_markdown_extras(source) {
            Some(extras) => extras,
            None => match gather_text_stats(source) {
                Some(stats) => Box::new(stats),
                None => crate::types::binary::info::gather_extras(magic_mime),
            },
        },
        FileType::Notebook => match crate::types::notebook::info_gather::gather_extras(source) {
            Some(extras) => extras,
            None => match gather_text_stats(source) {
                Some(stats) => Box::new(stats),
                None => crate::types::binary::info::gather_extras(magic_mime),
            },
        },
        FileType::Email(fmt) => match crate::types::email::info::gather_extras(source, *fmt) {
            Some(extras) => extras,
            None => match gather_text_stats(source) {
                Some(stats) => Box::new(stats),
                None => crate::types::binary::info::gather_extras(magic_mime),
            },
        },
        FileType::VObject(fmt) => match crate::types::vobject::info::gather_extras(source, *fmt) {
            Some(extras) => extras,
            None => match gather_text_stats(source) {
                Some(stats) => Box::new(stats),
                None => crate::types::binary::info::gather_extras(magic_mime),
            },
        },
        FileType::Svg => match (gather_text_stats(source), source.read_bytes()) {
            (Some(stats), Ok(bytes)) => {
                crate::types::svg::info_gather::gather_extras(stats, &bytes)
            }
            _ => crate::types::binary::info::gather_extras(magic_mime),
        },
        FileType::Structured(fmt) => match source.read_bytes() {
            Ok(bytes) => crate::types::structured::info::gather_extras(*fmt, &bytes),
            Err(_) => Box::new(crate::types::structured::info::StructuredInfo {
                format_name: crate::types::structured::info::format_name(*fmt),
                stats: None,
            }),
        },
        FileType::Html => match source.read_bytes() {
            Ok(bytes) => crate::types::structured::info::gather_extras(
                crate::input::detect::StructuredFormat::Xml,
                &bytes,
            ),
            Err(_) => Box::new(crate::types::structured::info::StructuredInfo {
                format_name: "HTML",
                stats: None,
            }),
        },
        FileType::Ebook(EbookFormat::Epub) => {
            crate::types::ebook::epub::info_gather::gather_extras(source)
        }
        FileType::Comic(fmt @ ComicFormat::Cbz) => {
            crate::types::comic::cbz::info_gather::gather_extras(source, *fmt)
        }
        FileType::Document(DocumentFormat::Docx) => {
            crate::types::document::docx::info_gather::gather_extras(source)
        }
        FileType::Document(DocumentFormat::Odt) => {
            crate::types::document::odt::info_gather::gather_extras(source)
        }
        FileType::Document(DocumentFormat::Rtf) => {
            crate::types::document::rtf::info_gather::gather_extras(source)
        }
        FileType::Pdf(flavor) => crate::types::pdf::info_gather::gather_extras(source, *flavor),
        FileType::PostScript(fmt) => crate::types::eps::info_gather::gather_extras(source, *fmt),
        FileType::Spreadsheet(fmt) => {
            crate::types::spreadsheet::info_gather::gather_extras(source, *fmt)
        }
        FileType::Image => crate::types::image::info_gather::gather_extras(source, magic_mime),
        FileType::Archive(fmt) => crate::types::archive::info::gather_extras(source, *fmt),
        FileType::Compressed(_) => crate::types::binary::info::gather_extras(magic_mime),
        FileType::DiskImage(fmt) => {
            crate::types::disk_image::info_gather::gather_extras(source, *fmt)
        }
        FileType::Audio(fmt) => crate::types::audio::info_gather::gather_extras(source, *fmt),
        FileType::Csv(fmt) => csv_gather(source, *fmt),
        FileType::Sqlite(_) => crate::types::sqlite::info_gather::gather_extras(source),
        FileType::Cert(fmt) => cert_gather(source, *fmt, magic_mime),
        FileType::Font(fmt) => font_gather(source, *fmt, magic_mime),
        FileType::ObjectFile => crate::types::objfile::info_gather::gather_extras(source),
        FileType::Classfile => crate::types::classfile::info_gather::gather_extras(source),
        FileType::Directory => match source {
            InputSource::File(path) => crate::types::directory::info::gather_extras(path),
            // A directory only ever reaches here via a real `File` source;
            // a virtual source can't name one.
            _ => crate::types::binary::info::gather_extras(magic_mime),
        },
        FileType::Binary => crate::types::binary::info::gather_extras(magic_mime),
    }
}

fn csv_gather(source: &InputSource, fmt: CsvFormat) -> Extras {
    match crate::types::csv::parse::CsvData::open(source, fmt) {
        Ok(data) => Box::new(crate::types::csv::info_gather::gather(&data, fmt)),
        Err(_) => crate::types::binary::info::gather_extras(None),
    }
}

/// Parse the source as a cert/key container. PEM reads the source text
/// (falling back to text stats / binary if it isn't valid UTF-8 — that
/// handles a `.pem` extension misapplied to a DER blob); DER reads the
/// raw bytes and decodes by structure. Capped at `LANG_STATS_BYTE_LIMIT`:
/// a multi-GB file claiming either format would otherwise pull the whole
/// blob into memory.
fn cert_gather(source: &InputSource, fmt: CertFormat, magic_mime: Option<&str>) -> Extras {
    if let Ok(bs) = source.open_byte_source()
        && bs.len() > LANG_STATS_BYTE_LIMIT
    {
        return crate::types::binary::info::gather_extras(magic_mime);
    }
    if fmt == CertFormat::Der {
        return match source.read_bytes() {
            Ok(der) => Box::new(crate::types::cert::info_gather::gather_der(&der)),
            Err(_) => crate::types::binary::info::gather_extras(magic_mime),
        };
    }
    let Some(text_stats) = gather_text_stats(source) else {
        return crate::types::binary::info::gather_extras(magic_mime);
    };
    let Ok(text) = source.read_text() else {
        return crate::types::binary::info::gather_extras(magic_mime);
    };
    Box::new(match fmt {
        CertFormat::Jwk => crate::types::cert::info_gather::gather_jwk(&text, text_stats),
        _ => crate::types::cert::info_gather::gather(&text, text_stats),
    })
}

/// Cap on bytes read for font parsing. The largest fonts in the wild —
/// Noto CJK supersets, Apple's San Francisco collection — sit around
/// 30–50 MB; 256 MB leaves comfortable headroom for the worst case
/// without putting an absurd buffer at the mercy of a hostile input.
const FONT_BYTE_LIMIT: u64 = 256 * 1024 * 1024;

fn font_gather(source: &InputSource, fmt: FontFormat, magic_mime: Option<&str>) -> Extras {
    if let Ok(bs) = source.open_byte_source()
        && bs.len() > FONT_BYTE_LIMIT
    {
        return crate::types::binary::info::gather_extras(magic_mime);
    }
    let Ok(bytes) = source.read_bytes() else {
        return crate::types::binary::info::gather_extras(magic_mime);
    };
    // Unwrap WOFF to its inner sfnt before parsing; bare sfnt borrows
    // through. A malformed wrapper falls back to the binary view.
    let Ok(sfnt) = crate::types::font::sfnt::decode(&bytes, fmt) else {
        return crate::types::binary::info::gather_extras(magic_mime);
    };
    Box::new(crate::types::font::info_gather::gather(&sfnt, fmt))
}
