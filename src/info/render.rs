use std::time::SystemTime;

use syntect::highlighting::Color;

use super::{FileExtras, FileInfo};
use crate::theme::{PeekTheme, lerp_color};

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
            bit_depth,
            hdr_format,
            frame_count,
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
            if *bit_depth > 0 {
                push_field(
                    &mut lines,
                    "Bit Depth",
                    &theme.paint_value(&format!("{bit_depth} bits/channel")),
                    theme,
                );
            }
            if let Some(hdr) = hdr_format {
                push_field(&mut lines, "HDR", &theme.paint_accent(hdr), theme);
            }
            if let Some(count) = frame_count {
                push_field(
                    &mut lines,
                    "Frames",
                    &theme.paint_value(&format!("{count} (animated)")),
                    theme,
                );
            }

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
