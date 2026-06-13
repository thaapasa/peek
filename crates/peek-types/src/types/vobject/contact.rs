//! vCard (`.vcf`) interpretation: render each `VCARD` as a grouped
//! contact card and summarise the address book for the Info sidecar.

use anyhow::Result;

use crate::input::InputSource;
use crate::theme::{PeekTheme, PeekThemeName, StyleMode};
use crate::viewer::modes::{ModeId, TextRenderer};

use super::datetime::format_datetime;
use super::line::{
    Component, ContentLine, format_list, parse_components, split_structured, unescape_text,
};
use super::render::{push_field, push_prose};

/// `TextRenderer` for a vCard document: one grouped block per contact.
pub(crate) struct ContactRenderer {
    source: InputSource,
}

impl ContactRenderer {
    pub(crate) fn new(source: InputSource) -> Self {
        Self { source }
    }
}

impl TextRenderer for ContactRenderer {
    fn label(&self) -> &'static str {
        "Contacts"
    }

    fn mode_id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn render(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        _theme_name: PeekThemeName,
        _style_mode: StyleMode,
    ) -> Result<Vec<String>> {
        let width = width.max(20);
        let bytes = self
            .source
            .read_bytes(crate::input::limits::Budget::Sidecar("vCard"))?;
        let text = String::from_utf8_lossy(&bytes);
        let cards: Vec<Component> = parse_components(&text)
            .into_iter()
            .filter(|c| c.name.eq_ignore_ascii_case("VCARD"))
            .collect();
        if cards.is_empty() {
            return Ok(vec![theme.paint_muted("[no contacts]")]);
        }

        let mut lines = Vec::new();
        for (i, card) in cards.iter().enumerate() {
            if i > 0 {
                lines.push(String::new());
            }
            render_card(&mut lines, card, theme, width);
        }
        Ok(lines)
    }
}

/// Render one `VCARD` block.
fn render_card(lines: &mut Vec<String>, card: &Component, theme: &PeekTheme, width: usize) {
    let name = card
        .value("FN")
        .or_else(|| card.prop("N").map(|p| name_from_n(&p.value)))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(no name)".into());
    lines.push(theme.paint_heading(&name));

    push_field(
        lines,
        "Nickname",
        card.value("NICKNAME").as_deref(),
        theme,
        width,
    );
    push_field(
        lines,
        "Org",
        card.prop("ORG").map(|p| org_line(&p.value)).as_deref(),
        theme,
        width,
    );
    push_field(lines, "Title", card.value("TITLE").as_deref(), theme, width);

    for email in card.props_named("EMAIL") {
        push_field(
            lines,
            "Email",
            Some(&annotate(&unescape_text(&email.value), email)),
            theme,
            width,
        );
    }
    for tel in card.props_named("TEL") {
        push_field(
            lines,
            "Phone",
            Some(&annotate(&phone_value(tel), tel)),
            theme,
            width,
        );
    }
    for adr in card.props_named("ADR") {
        push_field(
            lines,
            "Address",
            Some(&annotate(&adr_line(&adr.value), adr)),
            theme,
            width,
        );
    }

    push_field(lines, "Web", card.value("URL").as_deref(), theme, width);
    push_field(
        lines,
        "Birthday",
        card.prop("BDAY")
            .map(|p| format_datetime(&p.value))
            .as_deref(),
        theme,
        width,
    );
    push_field(
        lines,
        "Categories",
        card.prop("CATEGORIES")
            .map(|p| format_list(&p.value))
            .as_deref(),
        theme,
        width,
    );

    if let Some(note) = card.value("NOTE") {
        lines.push(String::new());
        push_prose(lines, &note, theme, width);
    }
}

/// Append a `(type, type)` suffix from the property's `TYPE` params, when
/// any are present. `"john@x (work)"`.
fn annotate(value: &str, prop: &ContentLine) -> String {
    let types = prop.param_values("TYPE");
    if types.is_empty() {
        return value.to_string();
    }
    format!("{value} ({})", types.join(", "))
}

/// Structured `N` (`Family;Given;Additional;Prefix;Suffix`) → a display
/// name `Prefix Given Additional Family Suffix`, used only when `FN` is
/// absent.
fn name_from_n(value: &str) -> String {
    let f = split_structured(value);
    let get = |i: usize| f.get(i).map(String::as_str).unwrap_or("");
    [get(3), get(1), get(2), get(0), get(4)]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Structured `ORG` (`Company;Department`) → `Company — Department`.
fn org_line(value: &str) -> String {
    split_structured(value)
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{2014} ")
}

/// Structured `ADR` (7 positional fields) → a one-line address. Skips the
/// rarely-populated po-box / extended fields.
fn adr_line(value: &str) -> String {
    let f = split_structured(value);
    // 0 po-box, 1 extended, 2 street, 3 locality, 4 region, 5 postal, 6 country
    let get = |i: usize| f.get(i).map(String::as_str).unwrap_or("").trim();
    [get(2), get(3), get(4), get(5), get(6)]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(", ")
}

/// A `TEL` value, stripping the `tel:` URI scheme vCard v4 uses.
fn phone_value(tel: &ContentLine) -> String {
    tel.value
        .strip_prefix("tel:")
        .or_else(|| tel.value.strip_prefix("TEL:"))
        .unwrap_or(&tel.value)
        .to_string()
}

/// Address-book metadata for the Info sidecar.
pub struct ContactSummary {
    pub contact_count: usize,
    /// vCard version of the first card (`3.0` / `4.0`), when declared.
    pub version: Option<String>,
}

/// Summarise a vCard document: contact count + the leading card's version.
pub fn summarize(text: &str) -> Option<ContactSummary> {
    let cards: Vec<Component> = parse_components(text)
        .into_iter()
        .filter(|c| c.name.eq_ignore_ascii_case("VCARD"))
        .collect();
    if cards.is_empty() {
        return None;
    }
    Some(ContactSummary {
        version: cards.first().and_then(|c| c.value("VERSION")),
        contact_count: cards.len(),
    })
}
