//! Render the spreadsheet workbook info section.

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::SpreadsheetFormat;
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

/// Typed `--info --json` encoding of the workbook section. `sheet_count`
/// is a raw number and `sheets` lists the names in workbook order;
/// `format` is a stable lowercase token; `error` is present only when the
/// workbook couldn't be opened (sheets are then empty).
pub fn json_section(info: &SpreadsheetInfo) -> (&'static str, serde_json::Value) {
    let mut obj = serde_json::json!({
        "format": format_token(info.format),
        "sheet_count": info.sheets.len(),
        "sheets": info.sheets,
    });
    if let Some(ref err) = info.error {
        obj["error"] = serde_json::json!(err);
    }

    let m = &info.metadata;
    let mut meta = serde_json::Map::new();
    if let Some(ref v) = m.title {
        meta.insert("title".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.creator {
        meta.insert("creator".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.subject {
        meta.insert("subject".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.keywords {
        meta.insert("keywords".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.created {
        meta.insert("created".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.modified {
        meta.insert("modified".into(), serde_json::json!(v));
    }
    if !meta.is_empty() {
        obj["metadata"] = serde_json::Value::Object(meta);
    }

    ("spreadsheet", obj)
}

fn format_token(format: SpreadsheetFormat) -> &'static str {
    match format {
        SpreadsheetFormat::Xlsx => "xlsx",
        SpreadsheetFormat::Xlsm => "xlsm",
        SpreadsheetFormat::Ods => "ods",
    }
}
