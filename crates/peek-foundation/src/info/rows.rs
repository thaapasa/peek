//! Hand-built row lists for irregular sections the `#[derive(InfoView)]` can't
//! model — enum-variant dispatch, or one print row mapping to several JSON
//! keys. An [`InfoRow`] carries an optional print label, an optional JSON key,
//! and a [`Value`] cell, so one `Vec<InfoRow>` feeds *both* outputs from a
//! single builder — the runtime complement to the derive.
//!
//! A row can be in both outputs ([`InfoRow::new`]), print-only
//! ([`InfoRow::print_only`] — e.g. a combined `RSA (2048 bit)` line whose parts
//! serialize as their own JSON-only rows), or JSON-only
//! ([`InfoRow::json_only`] — e.g. a `kind` tag, or a flat key with no
//! standalone print row). The cell is a [`Value`], so [`Value::split`] covers a
//! leaf whose print and JSON forms diverge (a join-string row that serializes
//! as an array).
//!
//! Prefer a struct + `#[derive(InfoView)]` when the layout is regular — it
//! keeps serde's compile-time field checking. Reach here only for the layouts
//! the derive genuinely can't express.

use std::borrow::Cow;

use peek_theme::PeekTheme;

use super::{InfoValue, Value, push_field};

/// One row of a hand-built section. See the [module docs](self).
pub struct InfoRow {
    print_label: Option<Cow<'static, str>>,
    json_key: Option<&'static str>,
    value: Value,
}

impl InfoRow {
    /// A row present in both outputs: print `label`, JSON key `key`.
    pub fn new(label: impl Into<Cow<'static, str>>, key: &'static str, value: Value) -> Self {
        InfoRow {
            print_label: Some(label.into()),
            json_key: Some(key),
            value,
        }
    }

    pub fn text(
        label: impl Into<Cow<'static, str>>,
        key: &'static str,
        value: impl Into<String>,
    ) -> Self {
        InfoRow {
            print_label: Some(label.into()),
            json_key: Some(key),
            value: Value::text(value.into()),
        }
    }

    // Both-output convenience constructors, one per `Value` kind — they keep
    // call sites to one line instead of `new(.., .., Value::kind(..))`.

    /// A both-output row whose cell is a grouped, colour-graded count.
    pub fn count(label: impl Into<Cow<'static, str>>, key: &'static str, n: u64) -> Self {
        Self::new(label, key, Value::count(n))
    }
    /// A both-output row whose cell is a plain signed integer.
    pub fn int(label: impl Into<Cow<'static, str>>, key: &'static str, n: i64) -> Self {
        Self::new(label, key, Value::int(n))
    }
    /// A both-output row printed in the muted colour, serialized as the string.
    pub fn muted(
        label: impl Into<Cow<'static, str>>,
        key: &'static str,
        s: impl Into<String>,
    ) -> Self {
        Self::new(label, key, Value::muted(s))
    }
    /// A both-output row printed in the accent colour, serialized as the string.
    pub fn accent(
        label: impl Into<Cow<'static, str>>,
        key: &'static str,
        s: impl Into<String>,
    ) -> Self {
        Self::new(label, key, Value::accent(s))
    }
    /// A both-output row printed in the warning colour, serialized as the string.
    pub fn warn(
        label: impl Into<Cow<'static, str>>,
        key: &'static str,
        s: impl Into<String>,
    ) -> Self {
        Self::new(label, key, Value::warn(s))
    }

    /// A print-only row (no JSON key).
    pub fn print_only(label: impl Into<Cow<'static, str>>, value: Value) -> Self {
        InfoRow {
            print_label: Some(label.into()),
            json_key: None,
            value,
        }
    }

    /// A JSON-only row (no print label).
    pub fn json_only(key: &'static str, value: Value) -> Self {
        InfoRow {
            print_label: None,
            json_key: Some(key),
            value,
        }
    }

    /// A JSON-only bool — e.g. a flag whose print row only shows when set.
    pub fn json_bool(key: &'static str, b: bool) -> Self {
        Self::json_only(key, Value::bool(b))
    }
    /// A JSON-only integer.
    pub fn json_int(key: &'static str, n: i64) -> Self {
        Self::json_only(key, Value::int(n))
    }
    /// A JSON-only string.
    pub fn json_text(key: &'static str, s: impl Into<String>) -> Self {
        Self::json_only(key, Value::text(s))
    }
}

/// Render a row list's print rows as themed `label  value` lines, for a caller
/// doing its own section framing (custom headers and the like).
pub fn push_rows(lines: &mut Vec<String>, rows: &[InfoRow], theme: &PeekTheme) {
    for row in rows {
        if let Some(label) = &row.print_label {
            push_field(lines, label, &row.value.render_value(theme), theme);
        }
    }
}

/// Collect a row list's keyed cells into a JSON object. Insertion order is
/// preserved — `serde_json` is built with `preserve_order`, so object keys
/// follow definition order rather than being alphabetized.
pub fn rows_to_json(rows: &[InfoRow]) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    for row in rows {
        if let Some(key) = row.json_key {
            map.insert(
                key.to_string(),
                serde_json::to_value(&row.value).expect("Value serializes"),
            );
        }
    }
    map
}
