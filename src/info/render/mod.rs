use syntect::highlighting::Color;

use super::{FileExtras, FileInfo};
use crate::theme::{PeekTheme, lerp_color};

mod file;

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
        FileExtras::Image(stats) => {
            crate::types::image::info_render::render_section(lines, stats, theme);
        }
        FileExtras::Text(stats) => {
            crate::types::text::info_render::render_section(lines, stats, theme);
        }
        FileExtras::Svg(stats) => {
            crate::types::svg::info_render::render_section(lines, stats, theme);
        }
        FileExtras::Structured(info) => {
            crate::types::structured::info::render_section(lines, info, theme);
        }
        FileExtras::Markdown(info) => {
            crate::types::markdown::info_render::render_section(lines, info, theme);
        }
        FileExtras::Sql(info) => {
            crate::types::sql::info_render::render_section(lines, info, theme);
        }
        FileExtras::Binary(info) => {
            crate::types::binary::info::render_section(lines, info, theme);
        }
        FileExtras::Archive(stats) => {
            crate::types::archive::info::render_section(lines, stats, theme);
        }
        FileExtras::DiskImage(info) => {
            crate::types::disk_image::info_render::render_section(lines, info, theme);
        }
        FileExtras::Ebook(stats) => {
            crate::types::ebook::epub::info_render::render_section(lines, stats, theme);
        }
        FileExtras::Comic(stats) => {
            crate::types::comic::cbz::info_render::render_section(lines, stats, theme);
        }
        FileExtras::Document(stats) => {
            crate::types::document::info_render::render_section(lines, stats, theme);
        }
        FileExtras::Pdf(stats) => {
            crate::types::pdf::info_render::render_section(lines, stats, theme);
        }
        FileExtras::Audio(stats) => {
            crate::types::audio::info_render::render_section(lines, stats, theme);
        }
        FileExtras::Csv(stats) => {
            crate::types::csv::info_render::render_section(lines, stats, theme);
        }
        FileExtras::Directory(stats) => {
            crate::types::directory::info::render_section(lines, stats, theme);
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
