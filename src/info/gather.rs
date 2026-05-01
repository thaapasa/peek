use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use image::ImageDecoder;

use super::{FileExtras, FileInfo, format_permissions_from_meta};
use crate::input::InputSource;
use crate::input::detect::{Detected, FileType, StructuredFormat};
use crate::input::mime;

/// Chunk size for streaming text-extras counting.
const TEXT_SCAN_CHUNK: usize = 64 * 1024;

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
    match file_type {
        FileType::SourceCode { .. } | FileType::Svg => {
            gather_text_extras_streaming(&InputSource::Stdin { data: Arc::clone(data) })
        }
        FileType::Structured(fmt) => FileExtras::Structured {
            format_name: match fmt {
                StructuredFormat::Json => "JSON",
                StructuredFormat::Yaml => "YAML",
                StructuredFormat::Toml => "TOML",
                StructuredFormat::Xml => "XML",
            },
        },
        FileType::Image => gather_image_extras_from_bytes(data, magic_mime),
        FileType::Binary => FileExtras::Binary,
    }
}

fn gather_image_extras_from_bytes(data: &Arc<[u8]>, magic_mime: Option<&str>) -> FileExtras {
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
        magic_mime,
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

fn gather_extras(path: &Path, file_type: &FileType, magic_mime: Option<&str>) -> FileExtras {
    match file_type {
        FileType::Image => gather_image_extras(path, magic_mime),
        FileType::SourceCode { .. } | FileType::Svg => {
            gather_text_extras_streaming(&InputSource::File(path.to_path_buf()))
        }
        FileType::Structured(fmt) => FileExtras::Structured {
            format_name: match fmt {
                StructuredFormat::Json => "JSON",
                StructuredFormat::Yaml => "YAML",
                StructuredFormat::Toml => "TOML",
                StructuredFormat::Xml => "XML",
            },
        },
        FileType::Binary => FileExtras::Binary,
    }
}

fn gather_image_extras(path: &Path, magic_mime: Option<&str>) -> FileExtras {
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
        magic_mime,
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

/// Stream the source in 64 KB chunks and count Unicode chars, words
/// (`split_whitespace`-equivalent), and lines (`str::lines`-equivalent)
/// without loading the whole file. Falls back to `FileExtras::Binary`
/// if the source isn't valid UTF-8.
fn gather_text_extras_streaming(source: &InputSource) -> FileExtras {
    let Ok(bs) = source.open_byte_source() else {
        return FileExtras::Binary;
    };
    let total = bs.len();

    let mut buf: Vec<u8> = Vec::with_capacity(TEXT_SCAN_CHUNK);
    let mut offset: u64 = 0;
    let mut chars = 0usize;
    let mut words = 0usize;
    let mut newlines = 0usize;
    // Whether we are currently inside a run of non-whitespace chars.
    // Carried across chunk boundaries so words split by a chunk edge
    // aren't double-counted.
    let mut in_word = false;
    // Last char observed across the whole stream — used to match
    // `str::lines()`: a final unterminated line still counts.
    let mut last_char: Option<char> = None;

    while offset < total {
        let want = ((total - offset) as usize).min(TEXT_SCAN_CHUNK);
        let read = match bs.read_range(offset, want) {
            Ok(r) => r,
            Err(_) => return FileExtras::Binary,
        };
        if read.is_empty() {
            break;
        }
        offset += read.len() as u64;
        buf.extend_from_slice(&read);

        // Find the longest valid UTF-8 prefix; the (≤3-byte) trailing
        // partial sequence stays in `buf` for the next chunk to complete.
        let valid_up_to = match std::str::from_utf8(&buf) {
            Ok(_) => buf.len(),
            Err(e) => {
                if e.error_len().is_some() {
                    return FileExtras::Binary;
                }
                e.valid_up_to()
            }
        };

        let s = std::str::from_utf8(&buf[..valid_up_to])
            .expect("valid_up_to slice is valid UTF-8 by construction");
        for ch in s.chars() {
            chars += 1;
            if ch == '\n' {
                newlines += 1;
            }
            let is_ws = ch.is_whitespace();
            if !is_ws && !in_word {
                words += 1;
                in_word = true;
            } else if is_ws {
                in_word = false;
            }
            last_char = Some(ch);
        }

        buf.drain(..valid_up_to);
    }

    // Anything left in `buf` at EOF was an incomplete trailing UTF-8
    // sequence — i.e. truncated text. Treat as binary.
    if !buf.is_empty() {
        return FileExtras::Binary;
    }

    // Match `str::lines()` semantics: empty input is 0 lines; otherwise a
    // file ending in `\n` has exactly `newlines` lines, and a file that
    // doesn't end in `\n` adds one for the final unterminated line.
    let line_count = match last_char {
        None => 0,
        Some('\n') => newlines,
        Some(_) => newlines + 1,
    };

    FileExtras::Text {
        line_count,
        word_count: words,
        char_count: chars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdin_source(text: &str) -> InputSource {
        InputSource::Stdin {
            data: Arc::from(text.as_bytes().to_vec().into_boxed_slice()),
        }
    }

    fn extras_text(s: &str) -> (usize, usize, usize) {
        match gather_text_extras_streaming(&stdin_source(s)) {
            FileExtras::Text {
                line_count,
                word_count,
                char_count,
            } => (line_count, word_count, char_count),
            other => panic!("expected Text extras, got {:?}", std::mem::discriminant(&other)),
        }
    }

    /// Streaming counts must match the old `str::lines/split_whitespace/chars`
    /// counts on the same input.
    fn assert_matches_std(s: &str) {
        let (lines, words, chars) = extras_text(s);
        assert_eq!(lines, s.lines().count(), "lines for {s:?}");
        assert_eq!(words, s.split_whitespace().count(), "words for {s:?}");
        assert_eq!(chars, s.chars().count(), "chars for {s:?}");
    }

    #[test]
    fn empty_input() {
        assert_matches_std("");
    }

    #[test]
    fn unterminated_final_line() {
        assert_matches_std("alpha\nbeta\ngamma");
    }

    #[test]
    fn terminated_final_line() {
        assert_matches_std("alpha\nbeta\ngamma\n");
    }

    #[test]
    fn blank_lines() {
        assert_matches_std("\n\n\n");
        assert_matches_std("a\n\nb\n");
    }

    #[test]
    fn crlf_lines() {
        assert_matches_std("a\r\nb\r\nc\r\n");
    }

    #[test]
    fn unicode_words_and_chars() {
        // Mixed Unicode, multi-byte chars at potential chunk edges.
        assert_matches_std("héllo wörld\nαβγ δεζ\n你好 世界\n");
    }

    #[test]
    fn invalid_utf8_returns_binary() {
        let bad = vec![0xff, 0xfe, 0xfd];
        let src = InputSource::Stdin {
            data: Arc::from(bad.into_boxed_slice()),
        };
        assert!(matches!(
            gather_text_extras_streaming(&src),
            FileExtras::Binary
        ));
    }

    #[test]
    fn truncated_utf8_returns_binary() {
        // First two bytes of a three-byte UTF-8 sequence (e.g. start of "你"),
        // unterminated.
        let truncated = vec![0xe4, 0xbd];
        let src = InputSource::Stdin {
            data: Arc::from(truncated.into_boxed_slice()),
        };
        assert!(matches!(
            gather_text_extras_streaming(&src),
            FileExtras::Binary
        ));
    }
}
