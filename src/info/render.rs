use std::time::SystemTime;

use syntect::highlighting::Color;

use super::time::format_time;
use super::{
    AnimationStats, Encoding, FileExtras, FileInfo, IndentStyle, LineEndings, LoopCount,
    StructuredStats, TextStats, TopLevelKind,
};
use crate::input::mime::{MimeCategory, MimeInfo};
use crate::theme::{PeekTheme, lerp_color};

/// Per-render options for the Info view.
#[derive(Clone, Copy, Default)]
pub struct RenderOptions {
    /// When true, show timestamps in UTC (ISO 8601 `...Z`). When false
    /// (default), show local time with `±HH:MM` offset.
    pub utc: bool,
}

/// Render file info as themed terminal lines.
pub fn render(info: &FileInfo, theme: &PeekTheme, opts: RenderOptions) -> Vec<String> {
    let mut lines = Vec::new();

    // Section: File
    push_section_header(&mut lines, "File", theme);
    push_field(
        &mut lines,
        "Name",
        &paint_filename(&info.file_name, theme),
        theme,
    );
    push_field(&mut lines, "Path", &paint_path(&info.path, theme), theme);
    push_field(
        &mut lines,
        "Size",
        &paint_size(info.size_bytes, theme),
        theme,
    );
    push_mime_field(&mut lines, &info.mimes, theme);
    if let Some(modified) = info.modified {
        push_field(
            &mut lines,
            "Modified",
            &paint_timestamp(modified, theme, opts.utc),
            theme,
        );
    }
    if let Some(created) = info.created {
        push_field(
            &mut lines,
            "Created",
            &paint_timestamp(created, theme, opts.utc),
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
            icc_profile,
            animation,
            exif,
            xmp,
        } => {
            lines.push(String::new());
            push_section_header(&mut lines, "Image", theme);
            push_field(
                &mut lines,
                "Dimensions",
                &paint_dimensions(*width, *height, theme),
                theme,
            );
            push_field(
                &mut lines,
                "Megapixels",
                &paint_megapixels(*width, *height, theme),
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
            if let Some(icc) = icc_profile {
                push_field(&mut lines, "ICC Profile", &theme.paint_value(icc), theme);
            }
            if let Some(hdr) = hdr_format {
                push_field(&mut lines, "HDR", &theme.paint_accent(hdr), theme);
            }
            if let Some(anim) = animation {
                push_animation(&mut lines, anim, theme);
            }

            if !exif.is_empty() {
                lines.push(String::new());
                push_section_header(&mut lines, "EXIF", theme);
                for (label, value) in exif {
                    push_field(&mut lines, label, &theme.paint_value(value), theme);
                }
            }

            if !xmp.is_empty() {
                lines.push(String::new());
                push_section_header(&mut lines, "XMP", theme);
                for (label, value) in xmp {
                    push_field(&mut lines, label, &theme.paint_value(value), theme);
                }
            }
        }
        FileExtras::Text(stats) => {
            lines.push(String::new());
            push_section_header(&mut lines, "Content", theme);
            push_text_stats(&mut lines, stats, theme);
        }
        FileExtras::Svg {
            text,
            view_box,
            declared_width,
            declared_height,
            path_count,
            group_count,
            rect_count,
            circle_count,
            text_count,
            has_script,
            has_external_href,
        } => {
            lines.push(String::new());
            push_section_header(&mut lines, "SVG", theme);
            if let Some(vb) = view_box {
                push_field(&mut lines, "viewBox", &theme.paint_value(vb), theme);
            }
            if let Some(w) = declared_width {
                push_field(&mut lines, "Width", &theme.paint_value(w), theme);
            }
            if let Some(h) = declared_height {
                push_field(&mut lines, "Height", &theme.paint_value(h), theme);
            }
            if *path_count > 0 {
                push_field(&mut lines, "Paths", &paint_count(*path_count, theme), theme);
            }
            if *group_count > 0 {
                push_field(&mut lines, "Groups", &paint_count(*group_count, theme), theme);
            }
            if *rect_count > 0 {
                push_field(&mut lines, "Rects", &paint_count(*rect_count, theme), theme);
            }
            if *circle_count > 0 {
                push_field(&mut lines, "Circles", &paint_count(*circle_count, theme), theme);
            }
            if *text_count > 0 {
                push_field(&mut lines, "Text Elems", &paint_count(*text_count, theme), theme);
            }
            if *has_script {
                push_field(
                    &mut lines,
                    "Script",
                    &theme.paint(" yes", theme.warning),
                    theme,
                );
            }
            if *has_external_href {
                push_field(
                    &mut lines,
                    "External ref",
                    &theme.paint(" yes", theme.warning),
                    theme,
                );
            }

            lines.push(String::new());
            push_section_header(&mut lines, "Source", theme);
            push_text_stats(&mut lines, text, theme);
        }
        FileExtras::Structured { format_name, stats } => {
            lines.push(String::new());
            push_section_header(&mut lines, "Format", theme);
            push_field(&mut lines, "Type", &theme.paint_accent(format_name), theme);
            if let Some(stats) = stats {
                push_structured_stats(&mut lines, stats, theme);
            }
        }
        FileExtras::Binary { format } => {
            if let Some(fmt) = format {
                lines.push(String::new());
                push_section_header(&mut lines, "Format", theme);
                push_field(&mut lines, "Type", &theme.paint_value(fmt), theme);
            }
        }
    }

    if !info.warnings.is_empty() {
        lines.push(String::new());
        push_section_header(&mut lines, "Warnings", theme);
        for w in &info.warnings {
            push_field(&mut lines, "Warning", &theme.paint(w, theme.warning), theme);
        }
    }

    lines
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

fn paint_megapixels(width: u32, height: u32, theme: &PeekTheme) -> String {
    let mp = (width as f64 * height as f64) / 1_000_000.0;
    let text = if mp < 1.0 {
        format!("{mp:.2} MP")
    } else {
        format!("{mp:.1} MP")
    };
    theme.paint(&text, theme.value)
}

fn push_animation(lines: &mut Vec<String>, anim: &AnimationStats, theme: &PeekTheme) {
    if let Some(count) = anim.frame_count {
        push_field(
            lines,
            "Frames",
            &theme.paint_value(&format!("{count} (animated)")),
            theme,
        );
    }
    if let Some(ms) = anim.total_duration_ms {
        let secs = ms as f64 / 1000.0;
        let label = if secs < 60.0 {
            format!("{secs:.2} s")
        } else {
            let mins = (secs / 60.0).floor();
            let rem = secs - mins * 60.0;
            format!("{mins:.0}m {rem:.2}s")
        };
        push_field(lines, "Duration", &theme.paint_value(&label), theme);
        if let Some(count) = anim.frame_count
            && ms > 0
        {
            let fps = count as f64 / (ms as f64 / 1000.0);
            push_field(
                lines,
                "Avg FPS",
                &theme.paint_muted(&format!("{fps:.1}")),
                theme,
            );
        }
    }
    if let Some(loops) = &anim.loop_count {
        let text = match loops {
            LoopCount::Infinite => "infinite".to_string(),
            LoopCount::Finite(0) => "infinite".to_string(),
            LoopCount::Finite(1) => "play once".to_string(),
            LoopCount::Finite(n) => format!("{n} times"),
        };
        push_field(lines, "Loop", &theme.paint_value(&text), theme);
    }
}

fn push_text_stats(lines: &mut Vec<String>, stats: &TextStats, theme: &PeekTheme) {
    push_field(lines, "Lines", &paint_count(stats.line_count, theme), theme);
    if stats.blank_lines > 0 {
        push_field(
            lines,
            "Blank Lines",
            &paint_count(stats.blank_lines, theme),
            theme,
        );
    }
    push_field(lines, "Words", &paint_count(stats.word_count, theme), theme);
    push_field(
        lines,
        "Characters",
        &paint_count(stats.char_count, theme),
        theme,
    );
    if stats.longest_line_chars > 0 {
        push_field(
            lines,
            "Longest Line",
            &paint_count(stats.longest_line_chars, theme),
            theme,
        );
    }
    push_field(
        lines,
        "Line Endings",
        &theme.paint_value(line_endings_label(stats.line_endings)),
        theme,
    );
    if let Some(indent) = stats.indent_style {
        push_field(
            lines,
            "Indent",
            &theme.paint_value(&indent_label(indent)),
            theme,
        );
    }
    push_field(
        lines,
        "Encoding",
        &theme.paint_muted(encoding_label(stats.encoding)),
        theme,
    );
    if let Some(shebang) = &stats.shebang {
        push_field(lines, "Shebang", &theme.paint_value(shebang), theme);
    }
}

fn line_endings_label(le: LineEndings) -> &'static str {
    match le {
        LineEndings::None => "none",
        LineEndings::Lf => "LF (\\n)",
        LineEndings::Crlf => "CRLF (\\r\\n)",
        LineEndings::Cr => "CR (\\r)",
        LineEndings::Mixed => "mixed",
    }
}

fn indent_label(style: IndentStyle) -> String {
    match style {
        IndentStyle::Tabs => "tabs".to_string(),
        IndentStyle::Spaces(n) => format!("{n} spaces"),
        IndentStyle::Mixed => "mixed".to_string(),
    }
}

fn encoding_label(enc: Encoding) -> &'static str {
    match enc {
        Encoding::Utf8 => "UTF-8",
        Encoding::Utf8Bom => "UTF-8 (BOM)",
        Encoding::Utf16Le => "UTF-16 LE",
        Encoding::Utf16Be => "UTF-16 BE",
    }
}

fn push_structured_stats(lines: &mut Vec<String>, stats: &StructuredStats, theme: &PeekTheme) {
    let (kind_label, count_label) = match &stats.top_level_kind {
        TopLevelKind::Object => ("Object", "Keys"),
        TopLevelKind::Array => ("Array", "Items"),
        TopLevelKind::Scalar => ("Scalar", "Items"),
        TopLevelKind::Table => ("Table", "Keys"),
        TopLevelKind::MultiDoc(_) => ("Multi-doc", "Top-level"),
        TopLevelKind::Document => ("Document", "Top-level"),
    };
    let kind_text = match &stats.top_level_kind {
        TopLevelKind::MultiDoc(n) => format!("Multi-doc ({n})"),
        _ => kind_label.to_string(),
    };
    push_field(lines, "Top-level", &theme.paint_value(&kind_text), theme);
    if stats.top_level_count > 0 {
        push_field(
            lines,
            count_label,
            &paint_count(stats.top_level_count, theme),
            theme,
        );
    }
    if stats.max_depth > 0 {
        push_field(
            lines,
            "Max Depth",
            &paint_count(stats.max_depth, theme),
            theme,
        );
    }
    if stats.total_nodes > 0 {
        push_field(
            lines,
            "Total Nodes",
            &paint_count(stats.total_nodes, theme),
            theme,
        );
    }
    if let Some(root) = &stats.xml_root {
        push_field(lines, "Root Element", &theme.paint_accent(root), theme);
    }
    if !stats.xml_namespaces.is_empty() {
        for (i, ns) in stats.xml_namespaces.iter().enumerate() {
            let label = if i == 0 { "Namespaces" } else { "" };
            push_field(lines, label, &theme.paint_muted(ns), theme);
        }
    }
}
