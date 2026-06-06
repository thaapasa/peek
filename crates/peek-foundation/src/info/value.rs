//! [`Value`] — a single info-field value that carries its *semantic* type,
//! not just a raw scalar.
//!
//! JSON serialization (this file) emits the machine form: a [`Value::Size`]
//! is a byte count, a [`Value::Timestamp`] an ISO-8601 UTC string, a
//! [`Value::Token`] a bare string. The semantic tag is what a future *print*
//! renderer will switch on to format and color — a `Size` as `"1 KiB"` on a
//! magnitude gradient, a `Count` with thousands separators, a `Timestamp`
//! age-colored. JSON can't tell `Size` from `Count` (both are numbers), but
//! print will, so the distinction is recorded at construction time now and
//! the print half is added later (see `docs/planned.md`).
//!
//! Per-type info sections build `#[derive(Serialize)]` view structs whose
//! "clear" fields are `Value`s; the complex, irregular sections use their own
//! dedicated `Serialize` types instead. Either way `InfoExtras::json_section`
//! returns `serde_json::to_value(view)`.

use std::time::SystemTime;

use serde::{Serialize, Serializer};

use super::time::format_time;

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
        }
    }
}

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
}
