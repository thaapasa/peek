//! [`Value`] — a single info-field value that carries its *semantic* type,
//! not just a raw scalar.
//!
//! One semantic tag drives both outputs from a single construction. JSON
//! (this file's `Serialize`) emits the machine form: a [`Value::Size`] is a
//! byte count, a [`Value::Timestamp`] an ISO-8601 UTC string, a
//! [`Value::Token`] a bare string. Print (the [`InfoValue`] impl in `render/`)
//! switches on the same tag to format and colour — a `Size` as `"1 KiB"` on a
//! magnitude gradient, a `Count` with thousands separators, a `Timestamp`
//! age-coloured. JSON can't tell `Size` from `Count` (both numbers), but print
//! can, so the distinction is recorded at construction time.
//!
//! [`Value::Split`] is the escape hatch for a leaf whose print text and JSON
//! value diverge and fit no semantic tag (prints `ELF`, serializes `"elf"`):
//! it carries both, pre-decided. See its variant docs.
//!
//! Per-type info sections build `#[derive(Serialize)]` view structs whose
//! "clear" fields are `Value`s; the complex, irregular sections use their own
//! dedicated `Serialize` types instead. Either way `InfoExtras::json_section`
//! returns `serde_json::to_value(view)`.

use std::time::SystemTime;

use serde::{Serialize, Serializer};

use super::InfoValue;
use super::time::format_time;
use crate::theme::PeekTheme;

/// One info-field value tagged with its semantic kind. See the module docs.
#[derive(Debug, Clone)]
pub enum Value {
    /// Byte count. JSON: number. Print (later): human size, e.g. `1 KiB`.
    Size(u64),
    /// Cardinal count. JSON: number. Print (later): grouped, e.g. `1,024`.
    Count(u64),
    /// Signed integer — version, id, offset. JSON: number.
    Int(i64),
    /// Ratio / fraction. JSON: number.
    Ratio(f64),
    /// Wall-clock instant. JSON: ISO-8601 UTC string. Print (later):
    /// local time, age-colored.
    Timestamp(SystemTime),
    /// Duration in milliseconds. JSON: number (ms).
    DurationMs(u64),
    /// Free text. JSON: string.
    Text(String),
    /// Machine token / enum label (already lowercased). JSON: string.
    Token(String),
    /// Boolean flag. JSON: bool.
    Bool(bool),
    /// Pre-decided field whose printed text and JSON value diverge and fit no
    /// semantic arm above — e.g. an enum printed `ELF` but serialized `"elf"`,
    /// or a duration shown `3:45` but stored as seconds. `text` is painted with
    /// `role` at render time (no theme here); `json` is emitted verbatim.
    ///
    /// Scalar leaves only. A composite JSON *object* still earns a typed
    /// `#[derive(Serialize)]` struct, so that editing the printed form can't
    /// silently desync the JSON shape — the divergence the one-view model
    /// exists to prevent. Reach for this only when print ≠ json *and* no
    /// semantic arm fits (don't `Split` a byte count — use [`Value::Size`], or
    /// the size gradient and typing are lost).
    Split {
        text: String,
        role: Role,
        json: serde_json::Value,
    },
}

/// Paint role for a [`Value::Split`]'s pre-formatted text — the same palette
/// the `painted_string!` newtypes carry, reified so `Split` can pick one.
#[derive(Debug, Clone, Copy)]
pub enum Role {
    /// Default value colour.
    Value,
    /// Muted — secondary metadata (dates, fallbacks).
    Muted,
    /// Accent — identifiers, format names, delimiters.
    Accent,
    /// Warning — problems, risky flags.
    Warn,
}

impl Role {
    /// Paint `text` in this role through `theme`.
    pub fn paint(self, theme: &PeekTheme, text: &str) -> String {
        match self {
            Role::Value => theme.paint_value(text),
            Role::Muted => theme.paint_muted(text),
            Role::Accent => theme.paint_accent(text),
            Role::Warn => theme.paint_warning(text),
        }
    }
}

impl Value {
    pub fn size(bytes: u64) -> Self {
        Value::Size(bytes)
    }
    pub fn count(n: u64) -> Self {
        Value::Count(n)
    }
    pub fn int(n: i64) -> Self {
        Value::Int(n)
    }
    pub fn ratio(r: f64) -> Self {
        Value::Ratio(r)
    }
    pub fn timestamp(t: SystemTime) -> Self {
        Value::Timestamp(t)
    }
    pub fn duration_ms(ms: u64) -> Self {
        Value::DurationMs(ms)
    }
    pub fn text(s: impl Into<String>) -> Self {
        Value::Text(s.into())
    }
    pub fn token(s: impl Into<String>) -> Self {
        Value::Token(s.into())
    }
    pub fn bool(b: bool) -> Self {
        Value::Bool(b)
    }
    /// Print `text` (painted with `role`), serialize `json` verbatim. The
    /// escape hatch for a scalar leaf whose print and JSON forms diverge.
    pub fn split(text: impl Into<String>, role: Role, json: serde_json::Value) -> Self {
        Value::Split {
            text: text.into(),
            role,
            json,
        }
    }
    /// Common [`split`](Value::split) case: a human label that serializes as a
    /// different string token, in the default value colour.
    pub fn labelled(label: impl Into<String>, token: impl Into<String>) -> Self {
        Value::Split {
            text: label.into(),
            role: Role::Value,
            json: serde_json::Value::String(token.into()),
        }
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Size(n) | Value::Count(n) | Value::DurationMs(n) => ser.serialize_u64(*n),
            Value::Int(n) => ser.serialize_i64(*n),
            Value::Ratio(r) => ser.serialize_f64(*r),
            Value::Timestamp(t) => ser.serialize_str(&format_time(*t, true)),
            Value::Text(s) | Value::Token(s) => ser.serialize_str(s),
            Value::Bool(b) => ser.serialize_bool(*b),
            Value::Split { json, .. } => json.serialize(ser),
        }
    }
}

/// A string field rendered in a *non-default* colour. Each newtype carries
/// the same machine value (serializes as the bare string) but paints
/// differently — secondary metadata muted, identifiers in accent, problems in
/// warning. Use these for fields whose print colour differs from the plain
/// value colour the blanket `String` impl gives.
macro_rules! painted_string {
    ($(#[$m:meta])* $name:ident => $paint:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone)]
        pub struct $name(pub String);

        impl From<String> for $name {
            fn from(s: String) -> Self {
                $name(s)
            }
        }
        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                $name(s.to_string())
            }
        }
        impl InfoValue for $name {
            fn render_value(&self, theme: &PeekTheme) -> String {
                theme.$paint(&self.0)
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
                ser.serialize_str(&self.0)
            }
        }
    };
}

painted_string!(
    /// Secondary text in the muted colour (dates, descriptions, fallbacks).
    Muted => paint_muted
);
painted_string!(
    /// Identifier-ish text in the accent colour (format names, delimiters).
    Accent => paint_accent
);
painted_string!(
    /// Problem text in the warning colour (parse errors, risky flags).
    Warn => paint_warning
);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn machine_forms() {
        assert_eq!(
            serde_json::to_value(Value::size(1024)).unwrap(),
            json!(1024)
        );
        assert_eq!(
            serde_json::to_value(Value::count(3400)).unwrap(),
            json!(3400)
        );
        assert_eq!(
            serde_json::to_value(Value::token("crlf")).unwrap(),
            json!("crlf")
        );
        assert_eq!(
            serde_json::to_value(Value::bool(true)).unwrap(),
            json!(true)
        );
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_736_951_400);
        assert_eq!(
            serde_json::to_value(Value::timestamp(t)).unwrap(),
            json!("2025-01-15T14:30:00Z")
        );
    }

    #[test]
    fn split_serializes_json_verbatim() {
        // The painted print text never leaks into JSON — only `json` does.
        assert_eq!(
            serde_json::to_value(Value::split("ELF", Role::Accent, json!("elf"))).unwrap(),
            json!("elf")
        );
        assert_eq!(
            serde_json::to_value(Value::split("0x10", Role::Value, json!(16))).unwrap(),
            json!(16)
        );
        // `labelled` is the string-label/string-token shorthand.
        assert_eq!(
            serde_json::to_value(Value::labelled("ELF", "elf")).unwrap(),
            json!("elf")
        );
    }
}
