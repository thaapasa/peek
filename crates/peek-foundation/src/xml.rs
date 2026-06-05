//! Shared XML helpers.
//!
//! quick-xml's `encoding` feature (pulled in transitively by calamine)
//! drops `Attribute::unescape_value` in favour of the decoder-aware
//! `decode_and_unescape_value`. Every XML peek parses is UTF-8 (OOXML,
//! ODF, EPUB, generic XML), so this decodes the raw attribute bytes as
//! UTF-8 and resolves character / entity references directly — no
//! `Decoder` in hand, and independent of which quick-xml features the
//! dependency graph happens to enable.

use quick_xml::events::attributes::Attribute;

/// UTF-8 attribute value with XML escapes resolved. `None` if the bytes
/// aren't valid UTF-8 or unescaping fails.
pub fn unescape_attr_value(attr: &Attribute<'_>) -> Option<String> {
    let raw = std::str::from_utf8(&attr.value).ok()?;
    quick_xml::escape::unescape(raw)
        .ok()
        .map(|cow| cow.into_owned())
}
