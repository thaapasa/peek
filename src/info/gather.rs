use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use image::ImageDecoder;

use super::{FileExtras, FileInfo, format_permissions_from_meta};
use crate::input::InputSource;
use crate::input::detect::{Detected, FileType, StructuredFormat};
use crate::input::mime;

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
    let extras = gather_extras(path, &detected.file_type);

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
    let extras = gather_extras_stdin(data, &detected.file_type);

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

fn gather_extras_stdin(data: &Arc<[u8]>, file_type: &FileType) -> FileExtras {
    match file_type {
        FileType::SourceCode { .. } | FileType::Svg => gather_text_extras_from_bytes(data),
        FileType::Structured(fmt) => FileExtras::Structured {
            format_name: match fmt {
                StructuredFormat::Json => "JSON",
                StructuredFormat::Yaml => "YAML",
                StructuredFormat::Toml => "TOML",
                StructuredFormat::Xml => "XML",
            },
        },
        FileType::Image => gather_image_extras_from_bytes(data),
        FileType::Binary => FileExtras::Binary,
    }
}

fn gather_image_extras_from_bytes(data: &Arc<[u8]>) -> FileExtras {
    let decoder = match image::ImageReader::new(std::io::Cursor::new(data.as_ref()))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_decoder().ok())
    {
        Some(d) => d,
        None => return FileExtras::Binary,
    };
    let (width, height) = decoder.dimensions();
    let ct = decoder.color_type();
    let color_type = format!("{ct:?}");
    let bit_depth = (ct.bits_per_pixel() / ct.channel_count() as u16) as u8;

    let hdr_format = detect_hdr_bytes(data);
    let frame_count = crate::viewer::image::animate::anim_frame_count(
        &InputSource::Stdin { data: Arc::clone(data) },
    );
    let exif = gather_exif_bytes(data);

    FileExtras::Image {
        width,
        height,
        color_type,
        bit_depth,
        hdr_format,
        frame_count,
        exif,
    }
}

fn gather_text_extras_from_bytes(data: &[u8]) -> FileExtras {
    let Ok(content) = std::str::from_utf8(data) else {
        return FileExtras::Binary;
    };
    FileExtras::Text {
        line_count: content.lines().count(),
        word_count: content.split_whitespace().count(),
        char_count: content.chars().count(),
    }
}

fn gather_extras(path: &Path, file_type: &FileType) -> FileExtras {
    match file_type {
        FileType::Image => gather_image_extras(path),
        FileType::SourceCode { .. } => gather_text_extras(path),
        FileType::Structured(fmt) => FileExtras::Structured {
            format_name: match fmt {
                StructuredFormat::Json => "JSON",
                StructuredFormat::Yaml => "YAML",
                StructuredFormat::Toml => "TOML",
                StructuredFormat::Xml => "XML",
            },
        },
        FileType::Svg => gather_text_extras(path),
        FileType::Binary => FileExtras::Binary,
    }
}

fn gather_image_extras(path: &Path) -> FileExtras {
    // ImageReader::open + into_decoder gives dimensions and color type
    // from the file's header without doing a full pixel decode — much
    // cheaper for large RAW/HEIC/etc.
    let decoder = match image::ImageReader::open(path)
        .ok()
        .and_then(|r| r.with_guessed_format().ok())
        .and_then(|r| r.into_decoder().ok())
    {
        Some(d) => d,
        None => return FileExtras::Binary,
    };
    let (width, height) = decoder.dimensions();
    let ct = decoder.color_type();
    let color_type = format!("{ct:?}");
    let bit_depth = (ct.bits_per_pixel() / ct.channel_count() as u16) as u8;

    let hdr_format = detect_hdr(path);
    let frame_count = crate::viewer::image::animate::anim_frame_count(
        &InputSource::File(path.to_path_buf()),
    );
    let exif = gather_exif(path);

    FileExtras::Image {
        width,
        height,
        color_type,
        bit_depth,
        hdr_format,
        frame_count,
        exif,
    }
}

/// Detect HDR format by scanning for known markers in file data.
fn detect_hdr(path: &Path) -> Option<String> {
    // Read enough to cover XMP metadata (typically in first 128KB)
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 128 * 1024];
    let n = std::io::Read::read(&mut file, &mut buf).ok()?;
    detect_hdr_bytes(&buf[..n])
}

fn detect_hdr_bytes(data: &[u8]) -> Option<String> {
    let slice = &data[..data.len().min(128 * 1024)];
    if slice.windows(6).any(|w| w == b"hdrgm:") {
        return Some("Ultra HDR (gain map)".to_string());
    }
    None
}

/// Extract interesting EXIF fields from an image file.
fn gather_exif(path: &Path) -> Vec<(String, String)> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut buf_reader = std::io::BufReader::new(file);
    exif_fields(&mut buf_reader)
}

fn gather_exif_bytes(data: &[u8]) -> Vec<(String, String)> {
    let mut cursor = std::io::Cursor::new(data);
    exif_fields(&mut cursor)
}

fn exif_fields<R: std::io::BufRead + std::io::Seek>(reader: &mut R) -> Vec<(String, String)> {
    let exif_reader = exif::Reader::new();
    let exif = match exif_reader.read_from_container(reader) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    // Fields to extract, in display order
    const FIELDS: &[(exif::Tag, &str)] = &[
        (exif::Tag::Make, "Camera Make"),
        (exif::Tag::Model, "Camera Model"),
        (exif::Tag::LensModel, "Lens"),
        (exif::Tag::ExposureTime, "Exposure"),
        (exif::Tag::FNumber, "Aperture"),
        (exif::Tag::PhotographicSensitivity, "ISO"),
        (exif::Tag::FocalLength, "Focal Length"),
        (exif::Tag::FocalLengthIn35mmFilm, "Focal Length (35mm)"),
        (exif::Tag::ExposureBiasValue, "Exposure Bias"),
        (exif::Tag::MeteringMode, "Metering"),
        (exif::Tag::Flash, "Flash"),
        (exif::Tag::WhiteBalance, "White Balance"),
        (exif::Tag::DateTimeOriginal, "Date Taken"),
        (exif::Tag::Software, "Software"),
        (exif::Tag::ImageDescription, "Description"),
        (exif::Tag::Artist, "Artist"),
        (exif::Tag::Copyright, "Copyright"),
        (exif::Tag::GPSLatitude, "GPS Latitude"),
        (exif::Tag::GPSLongitude, "GPS Longitude"),
        (exif::Tag::GPSAltitude, "GPS Altitude"),
    ];

    let mut result = Vec::new();
    for &(tag, label) in FIELDS {
        if let Some(field) = exif.get_field(tag, exif::In::PRIMARY) {
            let value = field.display_value().with_unit(&exif).to_string();
            let value = value.trim().to_string();
            if !value.is_empty() {
                result.push((label.to_string(), value));
            }
        }
    }
    result
}

fn gather_text_extras(path: &Path) -> FileExtras {
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return FileExtras::Binary,
    };

    let line_count = content.lines().count();
    let word_count = content.split_whitespace().count();
    let char_count = content.chars().count();

    FileExtras::Text {
        line_count,
        word_count,
        char_count,
    }
}
