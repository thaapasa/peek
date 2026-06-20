//! The email info section, driven by one [`EmailView`] that derives both
//! `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView) (themed
//! print). [`EmailInfo`] stays the gather struct; the view projects it.
//!
//! An `.mbox` populates only `message_count` (the gather blanks the per-message
//! fields), so the natural skips give just a `Messages` row. Header values are
//! truncated for the print row but serialized in full. Attachments print as a
//! count + size composite, serializing to `attachment_count` +
//! `attachment_bytes`.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use serde_json::json;

use crate::info::{InfoNode, Role, Value, format_size_human, paint_count};
use peek_theme::PeekTheme;

use super::EmailFormat;
use super::info::EmailInfo;

crate::info_section!(EmailInfo, EmailView, "email");

#[derive(Serialize, crate::info::InfoView)]
#[info(title_from = "section_title")]
struct EmailView {
    #[info(skip)]
    #[serde(rename = "format", serialize_with = "ser_format")]
    format: EmailFormat,
    #[info(label = "Messages")]
    #[serde(rename = "message_count", skip_serializing_if = "Option::is_none")]
    message_count: Option<Value>,
    #[info(label = "From")]
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<Value>,
    #[info(label = "To")]
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<Value>,
    #[info(label = "Cc")]
    #[serde(skip_serializing_if = "Option::is_none")]
    cc: Option<Value>,
    #[info(label = "Subject")]
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<Value>,
    #[info(label = "Date")]
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<Value>,
    #[info(label = "Message-ID")]
    #[serde(rename = "message_id", skip_serializing_if = "Option::is_none")]
    message_id: Option<Value>,
    #[info(nest)]
    #[serde(flatten)]
    attachments: Attachments,
}

impl EmailView {
    fn section_title(&self) -> &'static str {
        self.format.label()
    }
}

impl From<&EmailInfo> for EmailView {
    fn from(e: &EmailInfo) -> Self {
        // Truncated for the scannable print row, full in JSON.
        let header = |v: &Option<String>| {
            v.clone()
                .filter(|s| !s.is_empty())
                .map(|s| Value::split(truncate(&s, 100), Role::Value, json!(s)))
        };
        EmailView {
            format: e.format,
            message_count: e.message_count.map(|n| Value::count(n as u64)),
            from: header(&e.from),
            to: header(&e.to),
            cc: header(&e.cc),
            subject: header(&e.subject),
            date: e.date.map(Value::timestamp),
            message_id: header(&e.message_id),
            attachments: Attachments {
                count: e.attachment_count,
                bytes: e.attachment_bytes,
            },
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

/// Attachment tally. Print: a `count (size)` row when any. JSON:
/// `attachment_count` + `attachment_bytes` (always).
struct Attachments {
    count: usize,
    bytes: u64,
}

impl crate::info::InfoView for Attachments {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        if self.count == 0 {
            return Vec::new();
        }
        vec![InfoNode::Row {
            label: "Attachments".into(),
            value: format!(
                "{} {}",
                paint_count(self.count, theme),
                theme.paint_muted(&format!("({})", format_size_human(self.bytes)))
            ),
        }]
    }
}

impl Serialize for Attachments {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut st = ser.serialize_struct("attachments", 2)?;
        st.serialize_field("attachment_count", &self.count)?;
        st.serialize_field("attachment_bytes", &self.bytes)?;
        st.end()
    }
}

fn ser_format<S: Serializer>(fmt: &EmailFormat, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(match fmt {
        EmailFormat::Eml => "eml",
        EmailFormat::Mbox => "mbox",
    })
}
