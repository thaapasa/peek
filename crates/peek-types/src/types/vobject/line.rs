//! Shared content-line parser for iCalendar + vCard.
//!
//! Both formats serialise as RFC 5545 §3.1 / RFC 6350 §3.3 "content
//! lines": logical lines of the shape
//!
//! ```text
//! NAME(;PARAM=VALUE(,VALUE)*)* :VALUE
//! ```
//!
//! folded across physical lines (a continuation begins with a single
//! space or tab, which is stripped on unfolding). Components nest via
//! `BEGIN:X` / `END:X` markers. This module turns raw text into a tree of
//! [`Component`]s; the calendar / contact renderers interpret that tree.
//!
//! The grammar is small enough — and the rendering needs are specific
//! enough — that a hand-rolled parser beats pulling in a crate whose data
//! model we'd then have to translate. It also keeps the type self-contained
//! and dependency-free, matching peek's lean-runtime stance.

/// One parsed content line: a property name, its parameters, and the raw
/// (still-escaped) value text.
#[derive(Debug, Clone)]
pub struct ContentLine {
    /// Property name, upper-cased (`SUMMARY`, `DTSTART`, `EMAIL`, …).
    pub name: String,
    /// `(param-name, param-value)` pairs in source order. Param names are
    /// upper-cased; a multi-valued param (`TYPE=WORK,HOME`) keeps its
    /// comma-joined value verbatim — use [`ContentLine::param_values`] to
    /// split it.
    pub params: Vec<(String, String)>,
    /// Raw value text, escapes intact. Use [`unescape_text`] for a single
    /// text value or [`split_structured`] for `;`-delimited compound
    /// values (`N`, `ADR`).
    pub value: String,
}

impl ContentLine {
    /// First value of the named parameter (case-insensitive), if present.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// All values of the named parameter, flattened across both repeated
    /// params (`TYPE=WORK;TYPE=VOICE`) and comma-lists (`TYPE=WORK,VOICE`)
    /// and `"`-quoted forms (`TYPE="work,voice"`). Lower-cased for easy
    /// matching. Used for the vCard `TYPE` annotations.
    pub fn param_values(&self, name: &str) -> Vec<String> {
        self.params
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .flat_map(|(_, v)| v.trim_matches('"').split(','))
            .map(|s| s.trim().trim_matches('"').to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

/// A `BEGIN:NAME` … `END:NAME` block: its direct properties plus any
/// nested sub-components (a `VCALENDAR` holds `VEVENT` / `VTODO` /
/// `VTIMEZONE`; a `VEVENT` may hold a `VALARM`).
#[derive(Debug, Clone)]
pub struct Component {
    /// Component name, upper-cased (`VCALENDAR`, `VEVENT`, `VCARD`, …).
    pub name: String,
    /// Direct properties (everything that isn't a nested `BEGIN`/`END`).
    pub props: Vec<ContentLine>,
    /// Nested sub-components, in source order.
    pub children: Vec<Component>,
}

impl Component {
    /// First direct property with the given name (case-insensitive).
    pub fn prop(&self, name: &str) -> Option<&ContentLine> {
        self.props
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// Unescaped text value of the first matching property, if present and
    /// non-empty.
    pub fn value(&self, name: &str) -> Option<String> {
        self.prop(name)
            .map(|p| unescape_text(&p.value))
            .filter(|v| !v.is_empty())
    }

    /// Every direct property with the given name (case-insensitive) — for
    /// repeatable fields (`ATTENDEE`, `EMAIL`, `TEL`).
    pub fn props_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a ContentLine> {
        self.props
            .iter()
            .filter(move |p| p.name.eq_ignore_ascii_case(name))
    }
}

/// Parse text into the sequence of top-level components. iCalendar yields
/// a single `VCALENDAR`; vCard yields one component per `VCARD`. Lines
/// outside any `BEGIN`/`END` block are ignored (real files don't have
/// them, but a truncated upload might).
pub fn parse_components(text: &str) -> Vec<Component> {
    let lines = unfold(text);
    let mut iter = lines.into_iter().peekable();
    let mut roots = Vec::new();
    while let Some(line) = iter.peek() {
        if line.name.eq_ignore_ascii_case("BEGIN") {
            let name = line.value.trim().to_string();
            iter.next();
            roots.push(parse_component(&name, &mut iter));
        } else {
            iter.next();
        }
    }
    roots
}

/// Consume lines up to the matching `END:name`, building the component.
fn parse_component<I: Iterator<Item = ContentLine>>(
    name: &str,
    iter: &mut std::iter::Peekable<I>,
) -> Component {
    let mut comp = Component {
        name: name.to_ascii_uppercase(),
        props: Vec::new(),
        children: Vec::new(),
    };
    while let Some(line) = iter.next() {
        if line.name.eq_ignore_ascii_case("END") {
            // Closes this component (or an unbalanced parent — either way
            // we stop here and let the caller continue).
            break;
        }
        if line.name.eq_ignore_ascii_case("BEGIN") {
            let child_name = line.value.trim().to_string();
            comp.children.push(parse_component(&child_name, iter));
        } else {
            comp.props.push(line);
        }
    }
    comp
}

/// Unfold physical lines into logical content lines and parse each into a
/// [`ContentLine`]. A physical line starting with a space or tab is a
/// continuation of the previous logical line (the leading whitespace is
/// removed). Handles both CRLF (spec-mandated) and bare LF endings.
fn unfold(text: &str) -> Vec<ContentLine> {
    let mut logical: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(last) = logical.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        logical.push(line.to_string());
    }
    logical
        .iter()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| parse_line(l))
        .collect()
}

/// Parse one unfolded logical line into name / params / value. Returns
/// `None` for a line with no `:` separator (not a valid content line).
fn parse_line(line: &str) -> Option<ContentLine> {
    let colon = value_colon(line)?;
    let (head, value) = line.split_at(colon);
    let value = &value[1..]; // skip the ':'

    let mut parts = split_params(head);
    let name = parts.next()?.to_ascii_uppercase();
    let params = parts
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_ascii_uppercase(), v.to_string()))
        })
        .collect();

    Some(ContentLine {
        name,
        params,
        value: value.to_string(),
    })
}

/// Find the index of the `:` that separates the property part from the
/// value. A `:` inside a `"`-quoted parameter value doesn't count (vCard
/// v4 quotes params that contain structured punctuation).
fn value_colon(line: &str) -> Option<usize> {
    let mut in_quote = false;
    for (i, b) in line.bytes().enumerate() {
        match b {
            b'"' => in_quote = !in_quote,
            b':' if !in_quote => return Some(i),
            _ => {}
        }
    }
    None
}

/// Split the head (`NAME;P1=v1;P2=v2`) on `;` separators, skipping any
/// `;` inside a `"`-quoted param value.
fn split_params(head: &str) -> impl Iterator<Item = &str> {
    let mut in_quote = false;
    let mut start = 0;
    let mut out = Vec::new();
    for (i, b) in head.bytes().enumerate() {
        match b {
            b'"' => in_quote = !in_quote,
            b';' if !in_quote => {
                out.push(&head[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&head[start..]);
    out.into_iter()
}

/// Unescape a single text value per RFC 5545 §3.3.11 / RFC 6350 §3.4:
/// `\n` / `\N` → newline, `\,` → comma, `\;` → semicolon, `\\` →
/// backslash. Any other `\x` keeps the literal `x`.
pub fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Split a compound value (`N`, `ADR`) on unescaped `;` field separators,
/// then unescape each field. Empty fields are preserved (positional
/// fields matter: `;;Street;City;;;Country`).
pub fn split_structured(value: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                // Keep the escape intact for unescape_text below.
                cur.push(c);
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            ';' => {
                fields.push(unescape_text(&cur));
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    fields.push(unescape_text(&cur));
    fields
}

/// Split a comma-separated list value (`CATEGORIES`) on unescaped `,`,
/// unescaping each item. Returns the items re-joined with `", "` so a
/// terse `WORK,RELEASE` reads as `WORK, RELEASE`.
pub fn format_list(value: &str) -> String {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                cur.push(c);
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            ',' => {
                items.push(unescape_text(&cur));
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    items.push(unescape_text(&cur));
    items
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}
