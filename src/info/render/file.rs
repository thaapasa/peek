use std::time::SystemTime;

use syntect::highlighting::Color;

use super::super::FileInfo;
use super::super::time::format_time;
use super::{LABEL_WIDTH, push_field, push_section_header};
use crate::input::mime::{MimeCategory, MimeInfo};
use crate::theme::{PeekTheme, lerp_color};

pub(super) fn render_section(
    lines: &mut Vec<String>,
    info: &FileInfo,
    theme: &PeekTheme,
    utc: bool,
) {
    push_section_header(lines, "File", theme);
    push_field(
        lines,
        "Name",
        &paint_filename(&info.file_name, theme),
        theme,
    );
    push_field(lines, "Path", &paint_path(&info.path, theme), theme);
    push_field(lines, "Size", &paint_size(info.size_bytes, theme), theme);
    push_mime_field(lines, &info.mimes, theme);
    if let Some(modified) = info.modified {
        push_field(
            lines,
            "Modified",
            &paint_timestamp(modified, theme, utc),
            theme,
        );
    }
    if let Some(created) = info.created {
        push_field(
            lines,
            "Created",
            &paint_timestamp(created, theme, utc),
            theme,
        );
    }
    if let Some(ref perms) = info.permissions {
        push_field(
            lines,
            "Permissions",
            &paint_permissions(perms, theme),
            theme,
        );
    }
}

/// Render the MIME field as one or more lines: first line aligned with the
/// label, subsequent lines indented to the value column. Each entry shows
/// its MIME string plus a muted "(convention)"/"(vendor)"/etc. marker when
/// the type isn't formally registered.
fn push_mime_field(lines: &mut Vec<String>, mimes: &[MimeInfo], theme: &PeekTheme) {
    if mimes.is_empty() {
        return;
    }
    for (i, info) in mimes.iter().enumerate() {
        let painted = paint_mime(info, theme);
        if i == 0 {
            push_field(lines, "MIME", &painted, theme);
        } else {
            // Indent to align with the value column on the first line.
            lines.push(format!("  {}{}", " ".repeat(LABEL_WIDTH), painted));
        }
    }
}

fn paint_mime(info: &MimeInfo, theme: &PeekTheme) -> String {
    let value_color = match info.category {
        MimeCategory::Registered => theme.value,
        // Vendor types are still IANA-registered; show in value color too.
        MimeCategory::Vendor => theme.value,
        // Conventional / experimental / personal use a softer color to signal
        // they're not formally standardized.
        MimeCategory::Convention | MimeCategory::Experimental | MimeCategory::Personal => {
            lerp_color(theme.value, theme.muted, 0.3)
        }
    };
    let main = theme.paint(&info.mime, value_color);
    match info.category.marker() {
        Some(marker) => format!("{} {}", main, theme.paint_muted(marker)),
        None => main,
    }
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

fn format_size_display(bytes: u64) -> String {
    let exact = super::thousands_sep(bytes);
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

/// Paint timestamp with age-based color (recent = bright, old = dim).
fn paint_timestamp(time: SystemTime, theme: &PeekTheme, utc: bool) -> String {
    let color = timestamp_color(time, theme);
    let text = format_time(time, utc);
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

/// Paint permissions with per-character coloring. Handles both the
/// 10-char `ls -l` form (`drwxr-xr-x`) and the 9-char rwx-only form, plus
/// the Windows fallback strings (which don't get rwx separators).
fn paint_permissions(perms: &str, theme: &PeekTheme) -> String {
    // Group separators sit *after* the listed indices (e.g. 3 and 6 for
    // a 10-char string puts a divider after each rwx triplet).
    let chars: Vec<char> = perms.chars().collect();
    let separators: &[usize] = match chars.len() {
        10 => &[3, 6],
        9 => &[2, 5],
        _ => &[],
    };

    let mut result = String::new();
    for (i, ch) in chars.iter().enumerate() {
        let color = match ch {
            'r' => theme.value,
            'w' => theme.accent,
            'x' => theme.heading,
            // Special bits — accent so they pop. Capital S/T means the
            // execute bit is *not* set, which is more surprising than the
            // lowercase form, but a single color keeps the row legible.
            's' | 'S' | 't' | 'T' => theme.accent,
            // Type-prefix characters at index 0 of the 10-char form.
            'd' | 'l' | 'b' | 'c' | 'p' => theme.heading,
            '-' => lerp_color(theme.muted, theme.background, 0.3),
            _ => theme.foreground,
        };
        result.push_str(&theme.paint(&ch.to_string(), color));
        if separators.contains(&i) && i + 1 < chars.len() {
            result
                .push_str(&theme.paint("\u{2500}", lerp_color(theme.muted, theme.background, 0.5)));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{age_blend_factor, format_size_human};

    #[test]
    fn format_size_human_uses_correct_unit_at_boundaries() {
        assert_eq!(format_size_human(0), "0 B");
        assert_eq!(format_size_human(1), "1 B");
        assert_eq!(format_size_human(1023), "1023 B");
        assert_eq!(format_size_human(1024), "1.00 KiB");
        assert_eq!(format_size_human(1536), "1.50 KiB");
        assert_eq!(format_size_human(1024 * 1024 - 1), "1024.00 KiB");
        assert_eq!(format_size_human(1024 * 1024), "1.00 MiB");
        assert_eq!(format_size_human(1024 * 1024 * 1024), "1.00 GiB");
    }

    #[test]
    fn format_size_human_handles_petabyte_overflow() {
        // 2 PiB exceeds the explicit unit table; should fall through to PiB.
        let two_pib = 2u64 * 1024 * 1024 * 1024 * 1024 * 1024;
        assert_eq!(format_size_human(two_pib), "2.00 PiB");
    }

    #[test]
    fn age_blend_factor_is_monotonically_non_decreasing() {
        let samples = [
            0,
            3_000,
            3_600, // HOUR
            50_000,
            86_400, // DAY
            500_000,
            604_800, // WEEK
            2_000_000,
            2_592_000, // MONTH
            10_000_000,
            31_536_000, // YEAR
            100_000_000,
        ];
        let mut prev = age_blend_factor(samples[0]);
        for &s in &samples[1..] {
            let cur = age_blend_factor(s);
            assert!(cur >= prev, "non-monotonic at {s}: {prev} -> {cur}");
            prev = cur;
        }
    }

    #[test]
    fn age_blend_factor_hits_segment_boundaries() {
        // Curve is piecewise linear with segment endpoints at the named
        // constants. Boundary values are the documented anchors.
        assert_eq!(age_blend_factor(0), 0.0);
        assert!((age_blend_factor(3_600) - 0.0).abs() < 1e-6);
        assert!((age_blend_factor(86_400) - 0.15).abs() < 1e-6);
        assert!((age_blend_factor(604_800) - 0.30).abs() < 1e-6);
        assert!((age_blend_factor(2_592_000) - 0.45).abs() < 1e-6);
        assert!((age_blend_factor(31_536_000) - 0.60).abs() < 1e-6);
    }

    #[test]
    fn age_blend_factor_caps_at_60_percent() {
        assert_eq!(age_blend_factor(u64::MAX), 0.60);
    }
}
