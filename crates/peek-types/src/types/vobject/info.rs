//! vObject Info sidecar — gather + render in one file (tiny type).
//!
//! Surfaces the at-a-glance metadata the plan calls for: event / contact
//! counts, the calendar's date range, and the format version.

use serde::{Serialize, Serializer};

use crate::info::{Extras, InfoNode, paint_count, render_info};
use peek_io::InputSource;
use peek_theme::PeekTheme;

use super::VObjectFormat;
use super::calendar::{self, CalendarSummary};
use super::contact::{self, ContactSummary};

/// Cap on bytes parsed for the Info summary. iCalendar / vCard are
/// line-oriented text; a multi-GB file claiming the format would otherwise
/// pull the whole blob into memory just to count components. Above the cap
/// we fall back to plain text stats.
const SUMMARY_BYTE_LIMIT: u64 = 64 * 1024 * 1024;

/// Per-document metadata for the Info section.
pub struct VObjectInfo {
    pub format: VObjectFormat,
    pub detail: Detail,
}

/// Format-specific summary payload.
pub enum Detail {
    Calendar(CalendarSummary),
    Contact(ContactSummary),
}

/// Collect the Info sidecar. Returns `None` when the bytes don't parse as
/// the claimed format (gather falls back to text/binary).
pub fn gather_extras(source: &InputSource, fmt: VObjectFormat) -> Option<Extras> {
    if let Ok(bs) = source.open_byte_source()
        && bs.len() > SUMMARY_BYTE_LIMIT
    {
        return None;
    }
    let bytes = source
        .read_bytes(peek_io::limits::Budget::Unbounded(
            "gated by SUMMARY_BYTE_LIMIT above",
        ))
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let detail = match fmt {
        VObjectFormat::ICal => Detail::Calendar(calendar::summarize(&text)?),
        VObjectFormat::VCard => Detail::Contact(contact::summarize(&text)?),
    };
    Some(Box::new(VObjectInfo {
        format: fmt,
        detail,
    }))
}

/// Themed terminal vObject section.
pub fn render_section(lines: &mut Vec<String>, info: &VObjectInfo, theme: &PeekTheme) {
    render_info(lines, &VObjectView(info), theme);
}

/// Typed `--info --json` view of the vObject section, nested under
/// `"vobject"`. The `format` field is a stable token; the per-format detail
/// is a nested object.
pub fn json_section(info: &VObjectInfo) -> (&'static str, serde_json::Value) {
    (
        "vobject",
        serde_json::to_value(VObjectView(info)).expect("vobject info view serializes"),
    )
}

/// One-of view: the format-labelled block inlines the calendar / contact
/// rows for print, while JSON nests the detail under `calendar` / `contact`.
struct VObjectView<'a>(&'a VObjectInfo);

impl crate::info::InfoView for VObjectView<'_> {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let info = self.0;
        let mut body = Vec::new();
        let row = |label: &'static str, value: String| InfoNode::Row {
            label: label.into(),
            value,
        };
        match &info.detail {
            Detail::Calendar(cal) => {
                if let Some(name) = &cal.name {
                    body.push(row("Name", theme.paint_value(name)));
                }
                body.push(row("Events", paint_count(cal.event_count, theme)));
                if cal.todo_count > 0 {
                    body.push(row("Todos", paint_count(cal.todo_count, theme)));
                }
                if let Some((from, to)) = &cal.date_range {
                    let range = if from == to {
                        from.clone()
                    } else {
                        format!("{from} \u{2013} {to}")
                    };
                    body.push(row("Date range", theme.paint_value(&range)));
                }
                if let Some(version) = &cal.version {
                    body.push(row("Version", theme.paint_value(version)));
                }
                if let Some(product) = &cal.product {
                    body.push(row("Product", theme.paint_muted(product)));
                }
            }
            Detail::Contact(c) => {
                body.push(row("Contacts", paint_count(c.contact_count, theme)));
                if let Some(version) = &c.version {
                    body.push(row("Version", theme.paint_value(version)));
                }
            }
        }
        vec![InfoNode::Block {
            title: info.format.label().to_string(),
            body,
        }]
    }
}

impl Serialize for VObjectView<'_> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let info = self.0;
        let mut obj = serde_json::json!({ "format": format_token(info.format) });
        match &info.detail {
            Detail::Calendar(cal) => obj["calendar"] = calendar_json(cal),
            Detail::Contact(c) => obj["contact"] = contact_json(c),
        }
        obj.serialize(ser)
    }
}

fn format_token(fmt: VObjectFormat) -> &'static str {
    match fmt {
        VObjectFormat::ICal => "ical",
        VObjectFormat::VCard => "vcard",
    }
}

fn calendar_json(cal: &CalendarSummary) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "event_count": cal.event_count,
        "todo_count": cal.todo_count,
    });
    if let Some(name) = &cal.name {
        obj["name"] = serde_json::json!(name);
    }
    if let Some(version) = &cal.version {
        obj["version"] = serde_json::json!(version);
    }
    if let Some(product) = &cal.product {
        obj["product"] = serde_json::json!(product);
    }
    if let Some((from, to)) = &cal.date_range {
        obj["date_range"] = serde_json::json!({ "from": from, "to": to });
    }
    obj
}

fn contact_json(c: &ContactSummary) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "contact_count": c.contact_count,
    });
    if let Some(version) = &c.version {
        obj["version"] = serde_json::json!(version);
    }
    obj
}
