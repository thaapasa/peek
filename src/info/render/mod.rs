use syntect::highlighting::Color;

use super::{FileExtras, FileInfo};
use crate::theme::{PeekTheme, lerp_color};

mod file;

use crate::types::text::info_render::push_text_stats;

/// Per-render options for the Info view.
#[derive(Clone, Copy, Default)]
pub struct RenderOptions {
    /// When true, show timestamps in UTC (ISO 8601 `...Z`). When false
    /// (default), show local time with `±HH:MM` offset.
    pub utc: bool,
}

pub(super) const LABEL_WIDTH: usize = 14;

/// Render file info as themed terminal lines.
pub fn render(info: &FileInfo, theme: &PeekTheme, opts: RenderOptions) -> Vec<String> {
    let mut lines = Vec::new();

    file::render_section(&mut lines, info, theme, opts.utc);
    render_extras(&mut lines, &info.extras, theme);

    if !info.warnings.is_empty() {
        lines.push(String::new());
        push_section_header(&mut lines, "Warnings", theme);
        for w in &info.warnings {
            push_field(&mut lines, "Warning", &theme.paint(w, theme.warning), theme);
        }
    }

    lines
}

fn render_extras(lines: &mut Vec<String>, extras: &FileExtras, theme: &PeekTheme) {
    match extras {
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
            crate::types::image::info_render::render_section(
                lines,
                *width,
                *height,
                color_type,
                *bit_depth,
                hdr_format.as_deref(),
                icc_profile.as_deref(),
                animation.as_ref(),
                exif,
                xmp,
                theme,
            );
        }
        FileExtras::Text(stats) => {
            lines.push(String::new());
            push_section_header(lines, "Content", theme);
            push_text_stats(lines, stats, theme);
        }
        FileExtras::Svg {
            text: text_stats,
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
            animation,
            animation_warning,
        } => {
            crate::types::svg::info_render::render_section(
                lines,
                view_box.as_deref(),
                declared_width.as_deref(),
                declared_height.as_deref(),
                *path_count,
                *group_count,
                *rect_count,
                *circle_count,
                *text_count,
                *has_script,
                *has_external_href,
                animation.as_ref(),
                animation_warning.as_deref(),
                theme,
            );
            lines.push(String::new());
            push_section_header(lines, "Source", theme);
            push_text_stats(lines, text_stats, theme);
        }
        FileExtras::Structured { format_name, stats } => {
            crate::types::structured::info::render_section(
                lines,
                format_name,
                stats.as_ref(),
                theme,
            );
        }
        FileExtras::Binary { format } => {
            crate::types::binary::info::render_section(lines, format.as_deref(), theme);
        }
        FileExtras::Archive {
            format_name,
            entry_count,
            file_count,
            dir_count,
            total_uncompressed_size,
            error,
        } => {
            crate::types::archive::info::render_section(
                lines,
                format_name,
                *entry_count,
                *file_count,
                *dir_count,
                *total_uncompressed_size,
                error.as_deref(),
                theme,
            );
        }
    }
}

pub(crate) fn push_section_header(lines: &mut Vec<String>, title: &str, theme: &PeekTheme) {
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
pub(crate) fn push_field(
    lines: &mut Vec<String>,
    label: &str,
    colored_value: &str,
    theme: &PeekTheme,
) {
    let painted = theme.paint_label(label);
    let pad = if label.len() < LABEL_WIDTH {
        LABEL_WIDTH - label.len()
    } else {
        1
    };
    lines.push(format!("  {}{}{}", painted, " ".repeat(pad), colored_value));
}

/// Paint a count with magnitude-based intensity.
pub(crate) fn paint_count(count: usize, theme: &PeekTheme) -> String {
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

pub fn thousands_sep(n: u64) -> String {
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

#[cfg(test)]
mod tests {
    use super::thousands_sep;

    #[test]
    fn thousands_sep_inserts_commas_every_three_digits() {
        assert_eq!(thousands_sep(0), "0");
        assert_eq!(thousands_sep(1), "1");
        assert_eq!(thousands_sep(999), "999");
        assert_eq!(thousands_sep(1_000), "1,000");
        assert_eq!(thousands_sep(12_345), "12,345");
        assert_eq!(thousands_sep(1_234_567), "1,234,567");
    }

    #[test]
    fn thousands_sep_handles_u64_max() {
        assert_eq!(thousands_sep(u64::MAX), "18,446,744,073,709,551,615");
    }
}
