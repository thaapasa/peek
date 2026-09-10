//! Shared XML helpers.
//!
//! quick-xml decodes attribute values to `str` before we see them (every
//! XML peek parses is UTF-8 anyway), so what's left is resolving character /
//! entity references plus spec attribute-value normalisation (`\t` `\r` `\n`
//! → space). Helpers keep the `Option<String>` shape call sites want. Every
//! quick-xml package reader (docx/odt/pptx/odp/epub/dmg) matches element and
//! attribute names by local part — namespace prefixes vary per producer — so
//! that lookup lives here once.

use quick_xml::XmlVersion;
use quick_xml::events::BytesStart;
use quick_xml::events::attributes::Attribute;
use quick_xml::name::QName;

/// Attribute value with XML escapes resolved and whitespace normalised
/// (`\t` `\r` `\n` → space, per XML 1.0 §3.3.3). `None` if unescaping fails.
pub fn unescape_attr_value(attr: &Attribute<'_>) -> Option<String> {
    attr.normalized_value(XmlVersion::Implicit1_0)
        .ok()
        .map(|cow| cow.into_owned())
}

/// Local part of a qualified name: `w:p` → `p`.
pub fn local_name(name: QName<'_>) -> &str {
    name.local_name().into_inner()
}

/// First attribute whose local name is `want_local`, unescaped. Prefix
/// ignored — `r:embed` and `rel:embed` both match `embed`.
pub fn attr_local(e: &BytesStart<'_>, want_local: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|attr| attr.key.local_name().as_ref() == want_local)
        .and_then(|attr| unescape_attr_value(&attr))
}

/// Attribute matched by full prefixed key, unescaped — for attributes whose
/// local name collides (`r:id` vs the plain `id` on `<p:sldId>`).
pub fn attr_full(e: &BytesStart<'_>, want_key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|attr| attr.key.as_ref() == want_key)
        .and_then(|attr| unescape_attr_value(&attr))
}
