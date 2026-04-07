use std::fs;
use std::path::Path;
use std::time::SystemTime;

use anyhow::Result;
use syntect::highlighting::Color;

use crate::detect::{FileType, StructuredFormat};
use crate::theme::{PeekTheme, lerp_color};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Collected file metadata.
pub struct FileInfo {
    pub file_name: String,
    pub path: String,
    pub size_bytes: u64,
    pub mime_type: String,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub permissions: Option<String>,
    pub extras: FileExtras,
}

/// Type-specific metadata.
pub enum FileExtras {
    Image {
        width: u32,
        height: u32,
        color_type: String,
        exif: Vec<(String, String)>,
    },
    Text {
        line_count: usize,
        word_count: usize,
        char_count: usize,
    },
    Structured {
        format_name: &'static str,
    },
    Binary,
}

// ---------------------------------------------------------------------------
// Gather metadata
// ---------------------------------------------------------------------------

/// Gather file metadata for the given path and detected type.
pub fn gather(path: &Path, file_type: &FileType) -> Result<FileInfo> {
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
        FileType::Binary => FileExtras::Binary,
    }
}

fn gather_image_extras(path: &Path) -> FileExtras {
    let (width, height) = match image::image_dimensions(path) {
        Ok(dims) => dims,
        Err(_) => return FileExtras::Binary,
    };

    let color_type = image::open(path)
        .map(|img| format!("{:?}", img.color()))
        .unwrap_or_else(|_| "unknown".to_string());

    let exif = gather_exif(path);

    FileExtras::Image {
        width,
        height,
        color_type,
        exif,
    }
}

/// Extract interesting EXIF fields from an image file.
fn gather_exif(path: &Path) -> Vec<(String, String)> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut buf_reader = std::io::BufReader::new(file);
    let exif_reader = exif::Reader::new();
    let exif = match exif_reader.read_from_container(&mut buf_reader) {
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

// ---------------------------------------------------------------------------
// Render themed info
// ---------------------------------------------------------------------------

/// Render file info as themed terminal lines.
pub fn render(info: &FileInfo, theme: &PeekTheme) -> Vec<String> {
    let mut lines = Vec::new();

    // Section: File
    push_section_header(&mut lines, "File", theme);
    push_field(&mut lines, "Name", &paint_filename(&info.file_name, theme), theme);
    push_field(&mut lines, "Path", &paint_path(&info.path, theme), theme);
    push_field(
        &mut lines,
        "Size",
        &paint_size(info.size_bytes, theme),
        theme,
    );
    push_field(&mut lines, "MIME", &theme.paint_value(&info.mime_type), theme);
    if let Some(modified) = info.modified {
        push_field(
            &mut lines,
            "Modified",
            &paint_timestamp(modified, theme),
            theme,
        );
    }
    if let Some(created) = info.created {
        push_field(
            &mut lines,
            "Created",
            &paint_timestamp(created, theme),
            theme,
        );
    }
    if let Some(ref perms) = info.permissions {
        push_field(
            &mut lines,
            "Permissions",
            &paint_permissions(perms, theme),
            theme,
        );
    }

    // Type-specific section
    match &info.extras {
        FileExtras::Image {
            width,
            height,
            color_type,
            exif,
        } => {
            lines.push(String::new());
            push_section_header(&mut lines, "Image", theme);
            push_field(
                &mut lines,
                "Dimensions",
                &paint_dimensions(*width, *height, theme),
                theme,
            );
            push_field(&mut lines, "Color", &theme.paint_value(color_type), theme);

            if !exif.is_empty() {
                lines.push(String::new());
                push_section_header(&mut lines, "EXIF", theme);
                for (label, value) in exif {
                    push_field(&mut lines, label, &theme.paint_value(value), theme);
                }
            }
        }
        FileExtras::Text {
            line_count,
            word_count,
            char_count,
        } => {
            lines.push(String::new());
            push_section_header(&mut lines, "Content", theme);
            push_field(
                &mut lines,
                "Lines",
                &paint_count(*line_count, theme),
                theme,
            );
            push_field(
                &mut lines,
                "Words",
                &paint_count(*word_count, theme),
                theme,
            );
            push_field(
                &mut lines,
                "Characters",
                &paint_count(*char_count, theme),
                theme,
            );
            push_field(&mut lines, "Encoding", &theme.paint_muted("UTF-8"), theme);
        }
        FileExtras::Structured { format_name } => {
            lines.push(String::new());
            push_section_header(&mut lines, "Format", theme);
            push_field(&mut lines, "Type", &theme.paint_accent(format_name), theme);
        }
        FileExtras::Binary => {}
    }

    lines
}

const LABEL_WIDTH: usize = 14;

fn push_section_header(lines: &mut Vec<String>, title: &str, theme: &PeekTheme) {
    let rule_len = 40usize.saturating_sub(title.len() + 4);
    let rule = "\u{2500}".repeat(rule_len);
    lines.push(format!(
        "{} {} {}",
        theme.paint_muted("\u{2500}\u{2500}"),
        theme.paint_heading(title),
        theme.paint_muted(&rule),
    ));
}

/// Push a field with a themed label and a pre-colored value.
/// Guarantees at least one space between label and value.
fn push_field(lines: &mut Vec<String>, label: &str, colored_value: &str, theme: &PeekTheme) {
    let painted = theme.paint_label(label);
    let pad = if label.len() < LABEL_WIDTH {
        LABEL_WIDTH - label.len()
    } else {
        1
    };
    lines.push(format!("  {}{}{}", painted, " ".repeat(pad), colored_value));
}


// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

/// Paint filename with extension highlighted in accent.
fn paint_filename(name: &str, theme: &PeekTheme) -> String {
    if let Some(pos) = name.rfind('.') {
        let base = &name[..pos];
        let ext = &name[pos..];
        format!(
            "{}{}",
            theme.paint(base, theme.heading),
            theme.paint(ext, theme.accent)
        )
    } else {
        theme.paint(name, theme.heading)
    }
}

/// Paint path with directory components muted and final name highlighted.
fn paint_path(path: &str, theme: &PeekTheme) -> String {
    if let Some(pos) = path.rfind('/') {
        let dir = &path[..=pos];
        let name = &path[pos + 1..];
        format!(
            "{}{}",
            theme.paint(dir, theme.muted),
            theme.paint(name, theme.foreground)
        )
    } else {
        theme.paint(path, theme.foreground)
    }
}

/// Paint file size with color based on magnitude.
fn paint_size(bytes: u64, theme: &PeekTheme) -> String {
    let color = size_color(bytes, theme);
    let text = format_size_display(bytes);
    theme.paint(&text, color)
}

fn size_color(bytes: u64, theme: &PeekTheme) -> Color {
    if bytes == 0 {
        return theme.muted;
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1.0 {
        // < 1 KB: blend muted → value
        lerp_color(theme.muted, theme.value, kb as f32)
    } else if kb < 1024.0 {
        // 1 KB – 1 MB: value color
        theme.value
    } else if kb < 100.0 * 1024.0 {
        // 1 MB – 100 MB: value → accent
        let t = ((kb / 1024.0).ln() / 100_f64.ln()) as f32;
        lerp_color(theme.value, theme.accent, t.clamp(0.0, 1.0))
    } else {
        // > 100 MB: accent → warning
        let mb = kb / 1024.0;
        let t = ((mb / 100.0).clamp(1.0, 100.0).ln() / 100_f64.ln()) as f32;
        lerp_color(theme.accent, theme.warning, t.clamp(0.0, 1.0))
    }
}

/// Paint timestamp with age-based color (recent = bright, old = dim).
fn paint_timestamp(time: SystemTime, theme: &PeekTheme) -> String {
    let color = timestamp_color(time, theme);
    let text = format_time(time);
    theme.paint(&text, color)
}

fn timestamp_color(time: SystemTime, theme: &PeekTheme) -> Color {
    let age_secs = SystemTime::now()
        .duration_since(time)
        .map(|d| d.as_secs())
        .unwrap_or(u64::MAX);

    let t = age_blend_factor(age_secs);
    lerp_color(theme.value, theme.muted, t)
}

/// Smooth age-to-blend curve. Returns 0.0 for fresh, up to 0.6 for very old.
fn age_blend_factor(age_secs: u64) -> f32 {
    const HOUR: f64 = 3600.0;
    const DAY: f64 = 86400.0;
    const WEEK: f64 = 604800.0;
    const MONTH: f64 = 2_592_000.0;
    const YEAR: f64 = 31_536_000.0;

    let s = age_secs as f64;
    let t = if s < HOUR {
        0.0
    } else if s < DAY {
        0.15 * ((s - HOUR) / (DAY - HOUR))
    } else if s < WEEK {
        0.15 + 0.15 * ((s - DAY) / (WEEK - DAY))
    } else if s < MONTH {
        0.30 + 0.15 * ((s - WEEK) / (MONTH - WEEK))
    } else if s < YEAR {
        0.45 + 0.15 * ((s - MONTH) / (YEAR - MONTH))
    } else {
        0.60
    };

    t as f32
}

/// Paint permissions with per-character coloring.
fn paint_permissions(perms: &str, theme: &PeekTheme) -> String {
    let mut result = String::new();
    for (i, ch) in perms.chars().enumerate() {
        let color = match ch {
            'r' => theme.value,
            'w' => theme.accent,
            'x' => theme.heading,
            '-' => lerp_color(theme.muted, theme.background, 0.3),
            _ => theme.foreground,
        };
        result.push_str(&theme.paint(&ch.to_string(), color));
        // Add subtle separator between rwx groups
        if (i == 2 || i == 5) && i + 1 < perms.len() {
            result.push_str(&theme.paint("\u{2500}", lerp_color(theme.muted, theme.background, 0.5)));
        }
    }
    result
}

/// Paint a count with magnitude-based intensity.
fn paint_count(count: usize, theme: &PeekTheme) -> String {
    let color = count_color(count, theme);
    theme.paint(&thousands_sep(count as u64), color)
}

fn count_color(count: usize, theme: &PeekTheme) -> Color {
    if count == 0 {
        return theme.muted;
    }
    // Logarithmic: 1→0.4, 100→0.6, 10k→0.8, 1M→1.0 of value color
    let magnitude = (count as f64).log10();
    let t = (0.4 + 0.1 * magnitude).clamp(0.4, 1.0) as f32;
    lerp_color(theme.muted, theme.value, t)
}

/// Paint image dimensions with resolution-based coloring.
fn paint_dimensions(width: u32, height: u32, theme: &PeekTheme) -> String {
    let megapixels = (width as f64 * height as f64) / 1_000_000.0;
    let color = if megapixels < 0.5 {
        lerp_color(theme.muted, theme.value, (megapixels * 2.0) as f32)
    } else if megapixels < 8.0 {
        theme.value
    } else {
        let t = ((megapixels / 8.0).clamp(1.0, 10.0).ln() / 10_f64.ln()) as f32;
        lerp_color(theme.value, theme.accent, t)
    };
    theme.paint(&format!("{width} \u{00d7} {height}"), color)
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn format_size_display(bytes: u64) -> String {
    let exact = thousands_sep(bytes);
    let human = format_size_human(bytes);
    format!("{exact} bytes ({human})")
}

fn format_size_human(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    for unit in UNITS {
        if value < 1024.0 {
            return if *unit == "B" {
                format!("{value:.0} {unit}")
            } else {
                format!("{value:.2} {unit}")
            };
        }
        value /= 1024.0;
    }
    format!("{value:.2} PiB")
}

fn thousands_sep(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

fn format_time(time: SystemTime) -> String {
    let duration = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => return "unknown".to_string(),
    };

    let secs = duration.as_secs();

    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_date(days);

    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(unix)]
fn format_permissions_from_meta(meta: &fs::Metadata) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    Some(format_unix_permissions(mode))
}

#[cfg(not(unix))]
fn format_permissions_from_meta(meta: &fs::Metadata) -> Option<String> {
    let perms = meta.permissions();
    Some(if perms.readonly() {
        "read-only".to_string()
    } else {
        "read-write".to_string()
    })
}

#[cfg(unix)]
fn format_unix_permissions(mode: u32) -> String {
    let mut s = String::with_capacity(9);
    let flags = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    for (bit, ch) in flags {
        s.push(if mode & bit != 0 { ch } else { '-' });
    }
    s
}
