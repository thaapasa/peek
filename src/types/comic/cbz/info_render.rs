//! Render the comic info section.

use crate::info::{paint_count, push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;
use crate::types::comic::ComicStats;

pub fn render_section(lines: &mut Vec<String>, stats: &ComicStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, stats.format.label(), theme);

    if stats.page_count > 0 {
        push_field(lines, "Pages", &paint_count(stats.page_count, theme), theme);
    }
    if stats.total_image_bytes > 0 {
        let label = format!("{} bytes", thousands_sep(stats.total_image_bytes));
        push_field(lines, "Image bytes", &theme.paint_muted(&label), theme);
    }
}
