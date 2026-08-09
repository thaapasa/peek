//! The `FileType` → info-gather dispatch hub. `gather()` is the only public
//! entry point; it builds the common [`FileInfo`] frame (name / size / mtime /
//! permissions / MIME list / warnings / compression row) and delegates the
//! per-type [`Extras`] payload to the matching `peek_types::types::<type>`
//! module in [`gather_extras`]. Each type module owns its own format parsing;
//! this module only chooses which one to call and supplies the text/binary
//! fallback tail when no type-specific parse applies.

use std::fs;
use std::path::Path;

use anyhow::Result;
use peek_detect::mime;
use peek_detect::{
    ComicFormat, DecompressionContext, Detected, DocumentFormat, EbookFormat, FileType,
};
use peek_foundation::info::{CompressionInfo, Extras, FileInfo, format_permissions_from_meta};
use peek_io::InputSource;
use peek_types::types;

#[cfg(test)]
mod tests;

use peek_types::types::text::info_gather::gather_text_stats;

/// Generic per-source fallback: the streaming text stats when the bytes
/// are valid text, else the binary label. This is the bin's dispatch
/// policy — the type modules own their own format parsing, and only the
/// "nothing type-specific applied" tail lands here.
fn text_or_binary(source: &InputSource, magic_mime: Option<&str>) -> Extras {
    match gather_text_stats(source) {
        Some(stats) => Box::new(stats),
        None => types::binary::info::gather_extras(magic_mime),
    }
}

/// Resolve a type module's optional sidecar to concrete [`Extras`],
/// dropping to [`text_or_binary`] when the type-specific parse declined
/// (unparsable, or over its size cap).
fn or_text(extras: Option<Extras>, source: &InputSource, magic_mime: Option<&str>) -> Extras {
    extras.unwrap_or_else(|| text_or_binary(source, magic_mime))
}

/// Dispatch a SourceCode file to its language sidecar (SQL / CSS) by
/// syntax tag, falling back to the generic text/binary path.
fn source_code_extras(
    source: &InputSource,
    syntax: Option<&str>,
    magic_mime: Option<&str>,
) -> Extras {
    let sidecar = match syntax {
        Some("sql" | "ddl" | "dml" | "psql" | "pgsql") => {
            types::sql::info_gather::gather_extras(source)
        }
        // Plain CSS only — `.scss` / `.less` are different grammars and
        // keep the generic text-stats fallback.
        Some("css") => types::css::info_gather::gather_extras(source),
        _ => None,
    };
    or_text(sidecar, source, magic_mime)
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
        FileType::SourceCode { syntax } => {
            source_code_extras(source, syntax.as_deref(), magic_mime)
        }
        FileType::Markdown => or_text(
            types::markdown::info_gather::gather_extras(source),
            source,
            magic_mime,
        ),
        FileType::Notebook => or_text(
            types::notebook::info_gather::gather_extras(source),
            source,
            magic_mime,
        ),
        FileType::Email(fmt) => or_text(
            types::email::info::gather_extras(source, *fmt),
            source,
            magic_mime,
        ),
        FileType::VObject(fmt) => or_text(
            types::vobject::info::gather_extras(source, *fmt),
            source,
            magic_mime,
        ),
        FileType::Svg => match (
            gather_text_stats(source),
            source.read_bytes(peek_io::limits::Budget::Sidecar("SVG info")),
        ) {
            (Some(stats), Ok(bytes)) => types::svg::info_gather::gather_extras(stats, &bytes),
            _ => types::binary::info::gather_extras(magic_mime),
        },
        FileType::Structured(fmt) => {
            match source.read_bytes(peek_io::limits::Budget::Sidecar("structured info")) {
                Ok(bytes) => types::structured::info::gather_extras(*fmt, &bytes),
                Err(_) => Box::new(types::structured::info::StructuredInfo {
                    format_name: types::structured::info::format_name(*fmt),
                    stats: None,
                }),
            }
        }
        FileType::Html => match source.read_bytes(peek_io::limits::Budget::Sidecar("HTML info")) {
            Ok(bytes) => {
                types::structured::info::gather_extras(peek_detect::StructuredFormat::Xml, &bytes)
            }
            Err(_) => Box::new(types::structured::info::StructuredInfo {
                format_name: "HTML",
                stats: None,
            }),
        },
        FileType::Ebook(EbookFormat::Epub) => {
            types::ebook::epub::info_gather::gather_extras(source)
        }
        FileType::Comic(fmt @ ComicFormat::Cbz) => {
            types::comic::cbz::info_gather::gather_extras(source, *fmt)
        }
        FileType::Document(DocumentFormat::Docx) => {
            types::document::docx::info_gather::gather_extras(source)
        }
        FileType::Document(DocumentFormat::Odt) => {
            types::document::odt::info_gather::gather_extras(source)
        }
        FileType::Document(DocumentFormat::Rtf) => {
            types::document::rtf::info_gather::gather_extras(source)
        }
        FileType::Pdf(flavor) => types::pdf::info_gather::gather_extras(source, *flavor),
        FileType::PostScript(fmt) => types::eps::info_gather::gather_extras(source, *fmt),
        FileType::Spreadsheet(fmt) => types::spreadsheet::info_gather::gather_extras(source, *fmt),
        FileType::Presentation(fmt) => {
            types::presentation::info_gather::gather_extras(source, *fmt)
        }
        FileType::Image => types::image::info_gather::gather_extras(source, magic_mime),
        FileType::Archive(fmt) => types::archive::info::gather_extras(source, *fmt),
        FileType::Compressed(_) => types::binary::info::gather_extras(magic_mime),
        FileType::DiskImage(fmt) => types::disk_image::info_gather::gather_extras(source, *fmt),
        FileType::Audio(fmt) => types::audio::info_gather::gather_extras(source, *fmt),
        FileType::Csv(fmt) => types::csv::info_gather::gather_extras(source, *fmt),
        FileType::Sqlite(_) => types::sqlite::info_gather::gather_extras(source),
        FileType::Cert(fmt) => types::cert::info_gather::gather_extras(source, *fmt, magic_mime),
        FileType::Font(fmt) => types::font::info_gather::gather_extras(source, *fmt, magic_mime),
        FileType::ObjectFile => types::objfile::info_gather::gather_extras(source),
        FileType::Classfile => types::classfile::info_gather::gather_extras(source),
        FileType::DsStore => types::ds_store::info_gather::gather_extras(source),
        FileType::Directory => match source {
            InputSource::File(path) => types::directory::info::gather_extras(path),
            // A directory only ever reaches here via a real `File` source;
            // a virtual source can't name one.
            _ => types::binary::info::gather_extras(magic_mime),
        },
        FileType::Binary => types::binary::info::gather_extras(magic_mime),
    }
}
