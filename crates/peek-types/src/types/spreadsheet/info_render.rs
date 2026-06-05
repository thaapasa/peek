//! Render the spreadsheet workbook info section.

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::info::SpreadsheetInfo;

pub fn render_section(lines: &mut Vec<String>, info: &SpreadsheetInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, info.format.label(), theme);

    if let Some(err) = &info.error {
        push_field(lines, "Error", &theme.paint_warning(err), theme);
        return;
    }

    push_field(
        lines,
        "Sheets",
        &paint_count(info.sheets.len(), theme),
        theme,
    );
    if !info.sheets.is_empty() {
        push_field(
            lines,
            "Names",
            &theme.paint_muted(&info.sheets.join(", ")),
            theme,
        );
    }

    let m = &info.metadata;
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
}
