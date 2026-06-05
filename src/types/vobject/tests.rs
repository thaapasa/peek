//! Unit tests for the vObject parser, detection, and summaries.

use super::calendar;
use super::contact;
use super::datetime::{date_key, format_datetime};
use peek_detect::types::vobject as detect;

use super::VObjectFormat;
use super::line::{parse_components, split_structured, unescape_text};

const ICAL: &str = "\
BEGIN:VCALENDAR\r
VERSION:2.0\r
PRODID:-//Test//EN\r
X-WR-CALNAME:My Cal\r
BEGIN:VEVENT\r
UID:1@test\r
DTSTART;TZID=Europe/Helsinki:20260112T090000\r
DTEND;TZID=Europe/Helsinki:20260112T091500\r
SUMMARY:Standup\r
DESCRIPTION:Line one\\nLine two\\, still going\r
RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;COUNT=40\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:2@test\r
DTSTART;VALUE=DATE:20260203\r
SUMMARY:Launch\r
END:VEVENT\r
BEGIN:VTODO\r
UID:3@test\r
SUMMARY:Task\r
PERCENT-COMPLETE:60\r
END:VTODO\r
END:VCALENDAR\r
";

const VCARD: &str = "\
BEGIN:VCARD\r
VERSION:3.0\r
N:Smith;John;Quincy;Dr.;Jr.\r
FN:Dr. John Smith\r
ORG:Globex;Sales\r
EMAIL;TYPE=WORK:john@globex.example\r
TEL;TYPE=CELL,VOICE:+1 555 0142\r
ADR;TYPE=WORK:;;1 Market St;San Francisco;CA;94105;USA\r
END:VCARD\r
BEGIN:VCARD\r
VERSION:4.0\r
FN:Jane Doe\r
TEL;VALUE=uri:tel:+1-555-9999\r
END:VCARD\r
";

#[test]
fn unfolds_and_parses_components() {
    let roots = parse_components(ICAL);
    assert_eq!(roots.len(), 1);
    let cal = &roots[0];
    assert_eq!(cal.name, "VCALENDAR");
    assert_eq!(cal.children.len(), 3);
    assert_eq!(cal.value("X-WR-CALNAME").as_deref(), Some("My Cal"));
}

#[test]
fn folded_continuation_line_rejoins() {
    let text = "BEGIN:VCARD\nFN:Hello \n World\nEND:VCARD\n";
    let roots = parse_components(text);
    assert_eq!(roots[0].value("FN").as_deref(), Some("Hello World"));
}

#[test]
fn param_value_colon_does_not_split_quoted() {
    let roots = parse_components("BEGIN:VEVENT\nX-A;CN=\"a:b\":val\nEND:VEVENT\n");
    let line = roots[0].prop("X-A").unwrap();
    assert_eq!(line.param("CN"), Some("\"a:b\""));
    assert_eq!(line.value, "val");
}

#[test]
fn unescape_text_handles_specials() {
    assert_eq!(unescape_text("a\\nb\\,c\\;d\\\\e"), "a\nb,c;d\\e");
}

#[test]
fn split_structured_preserves_empty_fields() {
    let f = split_structured(";;Street;City;;;Country");
    assert_eq!(f, ["", "", "Street", "City", "", "", "Country"]);
}

#[test]
fn datetime_formats() {
    assert_eq!(format_datetime("20260112T090000"), "2026-01-12 09:00");
    assert_eq!(format_datetime("20260112T090000Z"), "2026-01-12 09:00 UTC");
    assert_eq!(format_datetime("20260203"), "2026-02-03");
    assert_eq!(format_datetime("1988-04-12"), "1988-04-12");
    // Unrecognised shape passes through untouched.
    assert_eq!(format_datetime("garbage"), "garbage");
    assert_eq!(date_key("20260112T090000Z").as_deref(), Some("2026-01-12"));
}

#[test]
fn calendar_summary_counts_and_range() {
    let s = calendar::summarize(ICAL).unwrap();
    assert_eq!(s.event_count, 2);
    assert_eq!(s.todo_count, 1);
    assert_eq!(s.name.as_deref(), Some("My Cal"));
    assert_eq!(s.version.as_deref(), Some("2.0"));
    assert_eq!(
        s.date_range,
        Some(("2026-01-12".to_string(), "2026-02-03".to_string()))
    );
}

#[test]
fn contact_summary_counts_and_version() {
    let s = contact::summarize(VCARD).unwrap();
    assert_eq!(s.contact_count, 2);
    assert_eq!(s.version.as_deref(), Some("3.0"));
}

#[test]
fn detect_by_extension() {
    assert_eq!(detect::format_from_ext("ics"), Some(VObjectFormat::ICal));
    assert_eq!(detect::format_from_ext("vcf"), Some(VObjectFormat::VCard));
    assert_eq!(detect::format_from_ext("txt"), None);
}

#[test]
fn detect_by_content() {
    assert_eq!(detect::sniff_text(ICAL), Some(VObjectFormat::ICal));
    assert_eq!(detect::sniff_text(VCARD), Some(VObjectFormat::VCard));
    assert_eq!(
        detect::sniff_text("\nBEGIN:VCARD\n"),
        Some(VObjectFormat::VCard)
    );
    assert_eq!(detect::sniff_text("hello world"), None);
}
