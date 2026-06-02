//! vObject detection: extension + content sniff.
//!
//! `.ics` / `.ical` is iCalendar; `.vcf` / `.vcard` is vCard. Both open
//! with a `BEGIN:VCALENDAR` / `BEGIN:VCARD` line (after optional leading
//! blank lines / BOM), which is a strong, unambiguous signature — no
//! false-positive guard is needed the way mbox / eml need one.

use super::format::VObjectFormat;

/// Map a lowercase extension to a vObject format.
pub fn format_from_ext(ext: &str) -> Option<VObjectFormat> {
    match ext {
        "ics" | "ical" | "ifb" => Some(VObjectFormat::ICal),
        "vcf" | "vcard" => Some(VObjectFormat::VCard),
        _ => None,
    }
}

/// Sniff a UTF-8 head for the opening `BEGIN:VCALENDAR` / `BEGIN:VCARD`
/// component marker. Case-insensitive on the value; tolerant of a leading
/// BOM and blank lines.
pub fn sniff_text(text: &str) -> Option<VObjectFormat> {
    let head = text
        .trim_start_matches('\u{feff}')
        .trim_start_matches(['\r', '\n', ' ', '\t']);
    let first = head.lines().next()?.trim();
    let value = first
        .strip_prefix("BEGIN:")
        .or_else(|| first.strip_prefix("begin:"))?;
    if value.eq_ignore_ascii_case("VCALENDAR") {
        Some(VObjectFormat::ICal)
    } else if value.eq_ignore_ascii_case("VCARD") {
        Some(VObjectFormat::VCard)
    } else {
        None
    }
}
