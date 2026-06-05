//! vObject Info sidecar — gather + render in one file (tiny type).
//!
//! Surfaces the at-a-glance metadata the plan calls for: event / contact
//! counts, the calendar's date range, and the format version.

use crate::info::{FileExtras, paint_count, push_field, push_section_header};
use crate::input::InputSource;
use crate::theme::PeekTheme;

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
pub fn gather_extras(source: &InputSource, fmt: VObjectFormat) -> Option<FileExtras> {
    if let Ok(bs) = source.open_byte_source()
        && bs.len() > SUMMARY_BYTE_LIMIT
    {
        return None;
    }
    let bytes = source.read_bytes().ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let detail = match fmt {
        VObjectFormat::ICal => Detail::Calendar(calendar::summarize(&text)?),
        VObjectFormat::VCard => Detail::Contact(contact::summarize(&text)?),
    };
    Some(FileExtras::VObject(VObjectInfo {
        format: fmt,
        detail,
    }))
}

/// Render the vObject Info section.
pub fn render_section(lines: &mut Vec<String>, info: &VObjectInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, info.format.label(), theme);

    match &info.detail {
        Detail::Calendar(cal) => render_calendar(lines, cal, theme),
        Detail::Contact(c) => render_contact(lines, c, theme),
    }
}

fn render_calendar(lines: &mut Vec<String>, cal: &CalendarSummary, theme: &PeekTheme) {
    if let Some(name) = &cal.name {
        push_field(lines, "Name", &theme.paint_value(name), theme);
    }
    push_field(lines, "Events", &paint_count(cal.event_count, theme), theme);
    if cal.todo_count > 0 {
        push_field(lines, "Todos", &paint_count(cal.todo_count, theme), theme);
    }
    if let Some((from, to)) = &cal.date_range {
        let range = if from == to {
            from.clone()
        } else {
            format!("{from} \u{2013} {to}")
        };
        push_field(lines, "Date range", &theme.paint_value(&range), theme);
    }
    if let Some(version) = &cal.version {
        push_field(lines, "Version", &theme.paint_value(version), theme);
    }
    if let Some(product) = &cal.product {
        push_field(lines, "Product", &theme.paint_muted(product), theme);
    }
}

fn render_contact(lines: &mut Vec<String>, c: &ContactSummary, theme: &PeekTheme) {
    push_field(
        lines,
        "Contacts",
        &paint_count(c.contact_count, theme),
        theme,
    );
    if let Some(version) = &c.version {
        push_field(lines, "Version", &theme.paint_value(version), theme);
    }
}
