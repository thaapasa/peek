//! Display and addressing for LMDB keys / values: table-cell text (text
//! when it reads as text, hex otherwise, native ints for INTEGERKEY /
//! INTEGERDUP) and the extract inner-path scheme.
//!
//! Inner path: `<db>/<key>` — plus `/<n>` (dup index) in a dupsort DB.
//! MAIN is `@main`. A segment that isn't safe as-is (non-UTF-8, control
//! chars, `/`, a leading `@` or `x:`, `.` / `..`, empty, or long) is
//! written `x:<hex>`, so any binary key round-trips.

use std::fmt::Write as _;

use bytes::Bytes;

use super::format::{MDB_INTEGERDUP, MDB_INTEGERKEY, int_le};

/// Inner-path segment naming MAIN; also its name in the listing and Info.
pub const MAIN_SEGMENT: &str = "@main";
const HEX_PREFIX: &str = "x:";
/// Longest segment kept as text; longer ones go hex.
const MAX_TEXT_SEGMENT: usize = 200;
/// Value bytes read for a cell preview.
pub const PREVIEW_BYTES: usize = 256;
/// Bytes shown before a hex preview truncates.
const HEX_PREVIEW_BYTES: usize = 32;

/// Key cell: a native int for INTEGERKEY, else text or hex.
pub fn key_cell(key: &[u8], db_flags: u16) -> String {
    if db_flags & MDB_INTEGERKEY != 0 && matches!(key.len(), 4 | 8) {
        return int_le(key).to_string();
    }
    text_or_hex(key, key.len() as u64)
}

/// Value cell from a preview of a `total`-byte value.
pub fn value_cell(preview: &[u8], total: u64, db_flags: u16, is_dup: bool) -> String {
    if is_dup && db_flags & MDB_INTEGERDUP != 0 && matches!(preview.len(), 4 | 8) {
        return int_le(preview).to_string();
    }
    text_or_hex(preview, total)
}

/// Text when `b` is UTF-8 without control chars (tab / newline / CR
/// allowed), else spaced hex. A truncated preview ends in `…`.
pub fn text_or_hex(b: &[u8], total: u64) -> String {
    let truncated = (b.len() as u64) < total;
    if let Some(text) = as_text(b, truncated) {
        let mut s = text.to_string();
        if truncated {
            s.push('…');
        }
        return s;
    }
    let shown = &b[..b.len().min(HEX_PREVIEW_BYTES)];
    let mut s = String::with_capacity(shown.len() * 3 + 1);
    for (i, byte) in shown.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{byte:02x}");
    }
    if (shown.len() as u64) < total {
        s.push_str(" …");
    }
    s
}

/// `b` as text, tolerating a char cut off by the preview boundary.
fn as_text(b: &[u8], truncated: bool) -> Option<&str> {
    let s = match std::str::from_utf8(b) {
        Ok(s) => s,
        Err(e) if truncated && e.error_len().is_none() => {
            std::str::from_utf8(&b[..e.valid_up_to()]).ok()?
        }
        Err(_) => return None,
    };
    let clean = s
        .chars()
        .all(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'));
    (clean && !s.is_empty() || b.is_empty()).then_some(s)
}

/// Encode a key or DB name as one inner-path segment.
pub fn encode_segment(b: &[u8]) -> String {
    let safe = std::str::from_utf8(b).ok().filter(|s| {
        !s.is_empty()
            && s.len() <= MAX_TEXT_SEGMENT
            && *s != "."
            && *s != ".."
            && !s.starts_with('@')
            && !s.starts_with(HEX_PREFIX)
            && !s.chars().any(|c| c == '/' || c == '\\' || c.is_control())
    });
    match safe {
        Some(s) => s.to_string(),
        None => {
            let mut out = String::with_capacity(HEX_PREFIX.len() + b.len() * 2);
            out.push_str(HEX_PREFIX);
            for byte in b {
                let _ = write!(out, "{byte:02x}");
            }
            out
        }
    }
}

/// Inverse of [`encode_segment`].
pub fn decode_segment(seg: &str) -> Option<Bytes> {
    let Some(hex) = seg.strip_prefix(HEX_PREFIX) else {
        return Some(Bytes::copy_from_slice(seg.as_bytes()));
    };
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()
        .map(Bytes::from)
}

/// DB segment: `@main` for MAIN, else the encoded name.
pub fn db_segment(name: Option<&[u8]>) -> String {
    name.map_or_else(|| MAIN_SEGMENT.to_string(), encode_segment)
}

/// Inner path of one row.
pub fn row_path(db_seg: &str, key: &[u8], dup: Option<u64>) -> String {
    let mut path = format!("{db_seg}/{}", encode_segment(key));
    if let Some(n) = dup {
        let _ = write!(path, "/{n}");
    }
    path
}

/// A parsed row inner path.
pub struct RowPath {
    /// `None` = MAIN.
    pub db: Option<Bytes>,
    pub key: Bytes,
    pub dup: Option<u64>,
}

pub fn parse_row_path(path: &str) -> Option<RowPath> {
    let mut parts = path.split('/');
    let db_seg = parts.next()?;
    let key = decode_segment(parts.next()?)?;
    let dup = match parts.next() {
        Some(n) => Some(n.parse().ok()?),
        None => None,
    };
    if parts.next().is_some() {
        return None;
    }
    let db = if db_seg == MAIN_SEGMENT {
        None
    } else {
        Some(decode_segment(db_seg)?)
    };
    Some(RowPath { db, key, dup })
}

/// File name offered when saving a value: the key as text when it's a
/// tame short name, else `value`.
pub fn suggested_name(key: &[u8]) -> String {
    let name: String = std::str::from_utf8(key)
        .unwrap_or("")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    let name = name.trim_matches('.');
    if name.is_empty() || name.chars().all(|c| c == '_') {
        "value".to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_values_stay_text_binary_goes_hex() {
        assert_eq!(text_or_hex(b"hello", 5), "hello");
        assert_eq!(text_or_hex(b"hello", 9), "hello…");
        assert_eq!(text_or_hex(&[0, 1, 0xff], 3), "00 01 ff");
        assert_eq!(text_or_hex(&[0; 40], 40).matches(' ').count(), 32);
        // A multi-byte char cut by the preview boundary still reads as text.
        let cut = &"café".as_bytes()[..4];
        assert_eq!(text_or_hex(cut, 5), "caf…");
        assert_eq!(text_or_hex(b"", 0), "");
    }

    #[test]
    fn integer_keys_and_dups_decode() {
        let k = 1_760_000_000u64.to_le_bytes();
        assert_eq!(key_cell(&k, MDB_INTEGERKEY), "1760000000");
        assert_eq!(key_cell(b"abc", MDB_INTEGERKEY), "abc", "wrong width");
        assert_eq!(
            value_cell(&7u32.to_le_bytes(), 4, MDB_INTEGERDUP, true),
            "7"
        );
    }

    #[test]
    fn segments_round_trip() {
        for raw in [
            &b"user:00001"[..],
            b"",
            b"a/b",
            b"@main",
            b"x:41",
            b"..",
            b"\x00\xff",
            "caf\u{e9}".as_bytes(),
        ] {
            let seg = encode_segment(raw);
            assert!(!seg.contains('/'), "{seg}");
            assert_eq!(&decode_segment(&seg).unwrap()[..], raw, "{seg}");
        }
        assert_eq!(encode_segment(b"users"), "users");
        assert_eq!(encode_segment(b"a/b"), "x:612f62");
        assert!(decode_segment("x:abc").is_none());
    }

    #[test]
    fn row_paths_round_trip() {
        let p = parse_row_path(&row_path("@main", b"k/1", None)).unwrap();
        assert!(p.db.is_none());
        assert_eq!(&p.key[..], b"k/1");
        assert_eq!(p.dup, None);

        let p = parse_row_path(&row_path("tags", b"beta", Some(3))).unwrap();
        assert_eq!(p.db.as_deref(), Some(&b"tags"[..]));
        assert_eq!(p.dup, Some(3));

        assert!(parse_row_path("tags").is_none());
        assert!(parse_row_path("tags/a/b/c").is_none());
        assert!(parse_row_path("tags/a/notanumber").is_none());
    }

    #[test]
    fn suggested_names_are_tame() {
        assert_eq!(suggested_name(b"user:00001"), "user_00001");
        assert_eq!(suggested_name(b"\x00\x01"), "value");
        assert_eq!(suggested_name(b"../etc"), "_etc");
    }
}
