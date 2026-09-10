//! Shared XML helpers.
//!
//! quick-xml decodes attribute values to `str` before we see them (every
//! XML peek parses is UTF-8 anyway), so all that's left is resolving
//! character / entity references. One helper keeps the `Option<String>`
//! shape call sites want.

use quick_xml::XmlVersion;
use quick_xml::events::attributes::Attribute;

/// Attribute value with XML escapes resolved. `None` if unescaping fails.
pub fn unescape_attr_value(attr: &Attribute<'_>) -> Option<String> {
    attr.normalized_value(XmlVersion::Implicit1_0)
        .ok()
        .map(|cow| cow.into_owned())
}
