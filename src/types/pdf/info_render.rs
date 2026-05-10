//! Render the PDF info section. Mirrors the shared document
//! info-render so the layout matches DOCX / RTF.

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::info::PdfStats;

pub fn render_section(lines: &mut Vec<String>, stats: &PdfStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "PDF", theme);

    if let Some(err) = &stats.error {
        push_field(lines, "Error", &theme.paint_warning(err), theme);
        return;
    }

    if !stats.pdf_version.is_empty() {
        push_field(
            lines,
            "Version",
            &theme.paint_value(&stats.pdf_version),
            theme,
        );
    }
    if stats.encrypted {
        push_field(lines, "Encrypted", &theme.paint_warning("yes"), theme);
    }

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
    if stats.page_count > 0 {
        push_field(lines, "Pages", &paint_count(stats.page_count, theme), theme);
    }
    if stats.attachment_count > 0 {
        push_field(
            lines,
            "Attachments",
            &paint_count(stats.attachment_count, theme),
            theme,
        );
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
