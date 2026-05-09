//! Render the EPUB info section.

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::info_gather::EpubStats;

pub fn render_section(lines: &mut Vec<String>, stats: &EpubStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "EPUB", theme);

    let m = &stats.metadata;
    if let Some(v) = &m.title {
        push_field(lines, "Title", &theme.paint_value(v), theme);
    }
    if let Some(v) = &m.creator {
        push_field(lines, "Author", &theme.paint_value(v), theme);
    }
    if let Some(v) = &m.language {
        push_field(lines, "Language", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.publisher {
        push_field(lines, "Publisher", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.date {
        push_field(lines, "Date", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.identifier {
        push_field(lines, "Identifier", &theme.paint_muted(v), theme);
    }
    if stats.chapter_count > 0 {
        push_field(
            lines,
            "Chapters",
            &paint_count(stats.chapter_count, theme),
            theme,
        );
    }
    if let Some(v) = &m.description {
        push_field(lines, "Description", &theme.paint_muted(v), theme);
    }
}
