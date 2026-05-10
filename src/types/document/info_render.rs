//! Render the shared document info section. Format label comes from
//! the [`DocumentFormat`](crate::input::detect::DocumentFormat) on the
//! stats so DOCX and RTF render through the same path.

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::info::DocumentStats;

pub fn render_section(lines: &mut Vec<String>, stats: &DocumentStats, theme: &PeekTheme) {
    lines.push(String::new());
    let header = match stats.format {
        crate::input::detect::DocumentFormat::Docx => "DOCX",
        crate::input::detect::DocumentFormat::Rtf => "RTF",
    };
    push_section_header(lines, header, theme);

    let m = &stats.metadata;
    if let Some(v) = &m.title {
        push_field(lines, "Title", &theme.paint_value(v), theme);
    }
    if let Some(v) = &m.creator {
        push_field(lines, "Author", &theme.paint_value(v), theme);
    }
    if let Some(v) = &m.subject {
        push_field(lines, "Subject", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.keywords {
        push_field(lines, "Keywords", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.created {
        push_field(lines, "Created", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.modified {
        push_field(lines, "Modified", &theme.paint_muted(v), theme);
    }
    if stats.paragraph_count > 0 {
        push_field(
            lines,
            "Paragraphs",
            &paint_count(stats.paragraph_count, theme),
            theme,
        );
    }
    if stats.word_count > 0 {
        push_field(lines, "Words", &paint_count(stats.word_count, theme), theme);
    }
    if stats.image_count > 0 {
        push_field(
            lines,
            "Images",
            &paint_count(stats.image_count, theme),
            theme,
        );
    }
    if let Some(v) = &m.description {
        push_field(lines, "Description", &theme.paint_muted(v), theme);
    }
}
