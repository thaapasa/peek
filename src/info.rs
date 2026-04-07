use std::fs;
use std::path::Path;
use std::time::SystemTime;

use anyhow::Result;

use crate::detect::{FileType, StructuredFormat};
use crate::theme::PeekTheme;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Collected file metadata.
pub struct FileInfo {
    pub file_name: String,
    pub path: String,
    pub size_bytes: u64,
    pub mime_type: Option<String>,
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

    // MIME type via infer (read first bytes)
    let mime_type = detect_mime(path);

    // Permissions (Unix only)
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
    let buf = fs::read(path).ok()?;
    let kind = infer::get(&buf)?;
    Some(kind.mime_type().to_string())
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

    FileExtras::Image {
        width,
        height,
        color_type,
    }
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
    push_field(&mut lines, "Name", &info.file_name, theme);
    push_field(&mut lines, "Path", &info.path, theme);
    push_field(&mut lines, "Size", &format_size_display(info.size_bytes), theme);
    if let Some(ref mime) = info.mime_type {
        push_field(&mut lines, "MIME", mime, theme);
    }
    if let Some(modified) = info.modified {
        push_field(&mut lines, "Modified", &format_time(modified), theme);
    }
    if let Some(created) = info.created {
        push_field(&mut lines, "Created", &format_time(created), theme);
    }
    if let Some(ref perms) = info.permissions {
        push_field(&mut lines, "Permissions", perms, theme);
    }

    // Type-specific section
    match &info.extras {
        FileExtras::Image { width, height, color_type } => {
            lines.push(String::new());
            push_section_header(&mut lines, "Image", theme);
            push_field(&mut lines, "Dimensions", &format!("{width} \u{00d7} {height}"), theme);
            push_field(&mut lines, "Color", color_type, theme);
        }
        FileExtras::Text { line_count, word_count, char_count } => {
            lines.push(String::new());
            push_section_header(&mut lines, "Content", theme);
            push_field(&mut lines, "Lines", &thousands_sep(*line_count as u64), theme);
            push_field(&mut lines, "Words", &thousands_sep(*word_count as u64), theme);
            push_field(&mut lines, "Characters", &thousands_sep(*char_count as u64), theme);
            push_field(&mut lines, "Encoding", "UTF-8", theme);
        }
        FileExtras::Structured { format_name } => {
            lines.push(String::new());
            push_section_header(&mut lines, "Format", theme);
            push_field(&mut lines, "Type", format_name, theme);
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

fn push_field(lines: &mut Vec<String>, label: &str, value: &str, theme: &PeekTheme) {
    lines.push(format!(
        "  {:<width$}{}",
        theme.paint_label(label),
        theme.paint_value(value),
        width = LABEL_WIDTH + ansi_overhead(theme, label),
    ));
}

/// The ANSI escape overhead added by paint_label for a given text length.
/// We need this to correctly pad the visible width.
fn ansi_overhead(theme: &PeekTheme, sample: &str) -> usize {
    theme.paint_label(sample).len() - sample.len()
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

    // Manual date/time calculation from Unix timestamp
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
    Some(if perms.readonly() { "read-only".to_string() } else { "read-write".to_string() })
}

#[cfg(unix)]
fn format_unix_permissions(mode: u32) -> String {
    let mut s = String::with_capacity(9);
    let flags = [
        (0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x'),
    ];
    for (bit, ch) in flags {
        s.push(if mode & bit != 0 { ch } else { '-' });
    }
    s
}
