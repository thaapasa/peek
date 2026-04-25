use std::fs;
use std::path::Path;

use anyhow::Result;

use super::{FileExtras, FileInfo, format_permissions_from_meta};
use crate::input::InputSource;
use crate::input::detect::{FileType, StructuredFormat};

/// Gather metadata for the given input source and detected type.
pub fn gather(source: &InputSource, file_type: &FileType) -> Result<FileInfo> {
    match source {
        InputSource::File(path) => gather_file(path, file_type),
        InputSource::Stdin { data } => Ok(gather_stdin(data, file_type)),
    }
}

fn gather_file(path: &Path, file_type: &FileType) -> Result<FileInfo> {
    let meta = fs::metadata(path)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let display_path = path.to_string_lossy().into_owned();

    // MIME type: try magic bytes first, fall back to extension-based detection
    let mime_type = detect_mime(path).unwrap_or_else(|| mime_from_type(file_type, path));

    let permissions = format_permissions_from_meta(&meta);
    let extras = gather_extras(path, file_type);

    Ok(FileInfo {
        file_name,
        path: display_path,
        size_bytes: meta.len(),
        mime_type,
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        permissions,
        extras,
    })
}

fn gather_stdin(data: &[u8], file_type: &FileType) -> FileInfo {
    let mime_type = infer::get(data)
        .map(|k| k.mime_type().to_string())
        .unwrap_or_else(|| mime_from_type_stdin(file_type));

    let extras = gather_extras_stdin(data, file_type);

    FileInfo {
        file_name: "<stdin>".to_string(),
        path: "<stdin>".to_string(),
        size_bytes: data.len() as u64,
        mime_type,
        modified: None,
        created: None,
        permissions: None,
        extras,
    }
}

fn mime_from_type_stdin(file_type: &FileType) -> String {
    match file_type {
        FileType::Structured(fmt) => match fmt {
            StructuredFormat::Json => "application/json",
            StructuredFormat::Yaml => "text/yaml",
            StructuredFormat::Toml => "application/toml",
            StructuredFormat::Xml => "application/xml",
        }
        .to_string(),
        FileType::SourceCode { .. } => "text/plain".to_string(),
        FileType::Svg => "image/svg+xml".to_string(),
        FileType::Image => "image/unknown".to_string(),
        FileType::Binary => "application/octet-stream".to_string(),
    }
}

fn gather_extras_stdin(data: &[u8], file_type: &FileType) -> FileExtras {
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

fn gather_image_extras_from_bytes(data: &[u8]) -> FileExtras {
    let img = match image::load_from_memory(data) {
        Ok(i) => i,
        Err(_) => return FileExtras::Binary,
    };
    let (width, height) = (img.width(), img.height());
    let color_type = format!("{:?}", img.color());
    let bit_depth =
        (img.color().bits_per_pixel() / img.color().channel_count() as u16) as u8;

    let hdr_format = detect_hdr_bytes(data);
    let frame_count = crate::viewer::image::animate::anim_frame_count(
        &InputSource::Stdin { data: data.to_vec() },
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

fn detect_mime(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = [0u8; 8192];
    let n = std::io::Read::read(&mut file, &mut buf).ok()?;
    let kind = infer::get(&buf[..n])?;
    Some(kind.mime_type().to_string())
}

fn mime_from_type(file_type: &FileType, path: &Path) -> String {
    match file_type {
        FileType::Image => {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            match ext {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                "svg" => "image/svg+xml",
                "ico" => "image/x-icon",
                "tiff" | "tif" => "image/tiff",
                _ => "image/unknown",
            }
            .to_string()
        }
        FileType::Structured(fmt) => match fmt {
            StructuredFormat::Json => "application/json",
            StructuredFormat::Yaml => "text/yaml",
            StructuredFormat::Toml => "application/toml",
            StructuredFormat::Xml => "application/xml",
        }
        .to_string(),
        FileType::SourceCode { syntax } => {
            let ext = syntax
                .as_deref()
                .or_else(|| path.extension().and_then(|e| e.to_str()))
                .unwrap_or("");
            // Use IANA-registered types where they exist (RFC 6648: no x- prefixes).
            // Languages without registered types fall back to text/plain.
            match ext {
                "js" | "mjs" | "cjs" => "text/javascript",
                "css" => "text/css",
                "html" | "htm" => "text/html",
                "md" | "markdown" => "text/markdown",
                "sql" => "application/sql",
                _ => "text/plain",
            }
            .to_string()
        }
        FileType::Svg => "image/svg+xml".to_string(),
        FileType::Binary => "application/octet-stream".to_string(),
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
    let (width, height) = match image::image_dimensions(path) {
        Ok(dims) => dims,
        Err(_) => return FileExtras::Binary,
    };

    let img = image::open(path);
    let color_type = img
        .as_ref()
        .map(|i| format!("{:?}", i.color()))
        .unwrap_or_else(|_| "unknown".to_string());
    let bit_depth = img
        .as_ref()
        .map(|i| i.color().bits_per_pixel() / i.color().channel_count() as u16)
        .unwrap_or(0) as u8;

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
