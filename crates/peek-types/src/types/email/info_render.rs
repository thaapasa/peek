//! Render the email Info section.

use crate::info::{format_size_human, paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::EmailFormat;
use super::info::EmailInfo;

pub fn render_section(lines: &mut Vec<String>, info: &EmailInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, info.format.label(), theme);

    if let Some(count) = info.message_count {
        push_field(lines, "Messages", &paint_count(count, theme), theme);
        return;
    }

    field(lines, "From", info.from.as_deref(), theme);
    field(lines, "To", info.to.as_deref(), theme);
    field(lines, "Cc", info.cc.as_deref(), theme);
    field(lines, "Subject", info.subject.as_deref(), theme);
    field(lines, "Date", info.date.as_deref(), theme);
    field(lines, "Message-ID", info.message_id.as_deref(), theme);

    if info.attachment_count > 0 {
        push_field(
            lines,
            "Attachments",
            &format!(
                "{} {}",
                paint_count(info.attachment_count, theme),
                theme.paint_muted(&format!("({})", format_size_human(info.attachment_bytes)))
            ),
            theme,
        );
    }
}

/// Emit a single header row, truncating long values so the Info screen
/// stays a scannable summary (the rendered view shows the full headers).
fn field(lines: &mut Vec<String>, label: &str, value: Option<&str>, theme: &PeekTheme) {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return;
    };
    push_field(
        lines,
        label,
        &theme.paint_value(&truncate(value, 100)),
        theme,
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

/// Typed `--info --json` encoding of the Email section. Header fields are
/// emitted in full (the truncation is a display concern only) and omitted when
/// absent; the format uses a stable lowercase token.
pub fn json_section(info: &EmailInfo) -> (&'static str, serde_json::Value) {
    let mut obj = serde_json::json!({
        "format": format_token(info.format),
        "attachment_count": info.attachment_count,
        "attachment_bytes": info.attachment_bytes,
    });
    if let Some(count) = info.message_count {
        obj["message_count"] = serde_json::json!(count);
    }
    if let Some(ref from) = info.from {
        obj["from"] = serde_json::json!(from);
    }
    if let Some(ref to) = info.to {
        obj["to"] = serde_json::json!(to);
    }
    if let Some(ref cc) = info.cc {
        obj["cc"] = serde_json::json!(cc);
    }
    if let Some(ref subject) = info.subject {
        obj["subject"] = serde_json::json!(subject);
    }
    if let Some(ref date) = info.date {
        obj["date"] = serde_json::json!(date);
    }
    if let Some(ref message_id) = info.message_id {
        obj["message_id"] = serde_json::json!(message_id);
    }
    ("email", obj)
}

fn format_token(fmt: EmailFormat) -> &'static str {
    match fmt {
        EmailFormat::Eml => "eml",
        EmailFormat::Mbox => "mbox",
    }
}
