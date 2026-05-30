//! Minimal standard-alphabet base64 decoder, shared crate-wide.
//!
//! The project ships no base64 crate (and pulls none in just for this),
//! so decode is hand-rolled: ~30 lines, whitespace-tolerant,
//! padding-agnostic. First consumer is the notebook viewer (image
//! outputs store their bytes as soft-wrapped base64); kept here at the
//! crate root, named `base64`, so any other type that needs to decode
//! an embedded blob can reuse it instead of rolling its own.

/// Map one base64 alphabet byte to its 6-bit value.
fn sextet(b: u8) -> Option<u32> {
    Some(match b {
        b'A'..=b'Z' => u32::from(b - b'A'),
        b'a'..=b'z' => u32::from(b - b'a') + 26,
        b'0'..=b'9' => u32::from(b - b'0') + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    })
}

/// Decode a base64 string. Whitespace (line wraps) and `=` padding are
/// skipped; any other non-alphabet byte makes the whole decode fail
/// (`None`) rather than silently producing garbage.
pub(crate) fn decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &b in s.as_bytes() {
        if b == b'=' || b.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | sextet(b)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Decoded byte length without allocating — for the listing size column.
/// Counts alphabet bytes (ignoring whitespace + padding); each carries
/// 6 bits, so the byte count is `chars * 6 / 8`.
pub(crate) fn decoded_len(s: &str) -> usize {
    let chars = s
        .bytes()
        .filter(|&b| b != b'=' && !b.is_ascii_whitespace())
        .count();
    chars * 6 / 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_known_vectors() {
        assert_eq!(decode("").unwrap(), b"");
        assert_eq!(decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        // A real 1x1 PNG starts with the PNG magic.
        let png = decode("iVBORw0KGgo=").unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn tolerates_whitespace_wraps() {
        assert_eq!(decode("aGVs\nbG8=").unwrap(), b"hello");
        assert_eq!(decode("  aGVsbG8=  ").unwrap(), b"hello");
    }

    #[test]
    fn rejects_non_alphabet() {
        assert!(decode("not base64!@#").is_none());
    }

    #[test]
    fn decoded_len_matches_decode() {
        for s in ["aGVsbG8=", "aGVsbG8gd29ybGQ=", "iVBORw0KGgo="] {
            assert_eq!(decoded_len(s), decode(s).unwrap().len(), "len for {s}");
        }
    }
}
