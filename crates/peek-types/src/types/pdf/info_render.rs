//! Render the PDF info section. Mirrors the shared document
//! info-render so the layout matches DOCX / RTF.

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::info::PdfStats;
use crate::types::pdf::PdfFlavor;

pub fn render_section(lines: &mut Vec<String>, stats: &PdfStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, stats.flavor.label(), theme);

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

/// Typed `--info --json` encoding of the PDF section. Counts are raw
/// numbers; `flavor` is a stable lowercase token; `error` is present only
/// when the document couldn't be opened (other fields are then default).
pub fn json_section(stats: &PdfStats) -> (&'static str, serde_json::Value) {
    let mut obj = serde_json::json!({
        "flavor": flavor_token(stats.flavor),
        "encrypted": stats.encrypted,
    });
    if let Some(ref err) = stats.error {
        obj["error"] = serde_json::json!(err);
    }
    if !stats.pdf_version.is_empty() {
        obj["pdf_version"] = serde_json::json!(stats.pdf_version);
    }
    if stats.page_count > 0 {
        obj["page_count"] = serde_json::json!(stats.page_count);
    }
    if stats.attachment_count > 0 {
        obj["attachment_count"] = serde_json::json!(stats.attachment_count);
    }
    if stats.image_count > 0 {
        obj["image_count"] = serde_json::json!(stats.image_count);
    }

    let m = &stats.metadata;
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
    if let Some(ref v) = m.description {
        meta.insert("description".into(), serde_json::json!(v));
    }
    if !meta.is_empty() {
        obj["metadata"] = serde_json::Value::Object(meta);
    }

    ("pdf", obj)
}

fn flavor_token(flavor: PdfFlavor) -> &'static str {
    match flavor {
        PdfFlavor::Pdf => "pdf",
        PdfFlavor::Illustrator => "illustrator",
    }
}
