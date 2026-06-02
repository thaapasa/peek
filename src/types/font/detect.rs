//! Detection contributions for font files. `format_from_ext` covers
//! filename routing (`classify_by_name`); `sniff_font_bytes` covers
//! content sniffing for unnamed sources (stdin, archive entries) and
//! for files whose extension lies.

use crate::types::font::format::FontFormat;

/// Map a lowercased filename extension to the font container format.
/// `.ttc` and `.otc` both route to [`FontFormat::Collection`] — the
/// container is the same regardless of whether the embedded faces are
/// TrueType or CFF outlines.
pub fn format_from_ext(ext: &str) -> Option<FontFormat> {
    Some(match ext {
        "ttf" => FontFormat::TrueType,
        "otf" => FontFormat::OpenType,
        "ttc" | "otc" => FontFormat::Collection,
        "woff" => FontFormat::Woff,
        _ => return None,
    })
}

/// Sniff the leading bytes of a source for an OpenType wrapper. Used
/// by the content-based detection path when extension routing doesn't
/// apply. Returns `None` if `head` is shorter than the 4-byte magic or
/// doesn't match a known signature.
pub fn sniff_font_bytes(head: &[u8]) -> Option<FontFormat> {
    if head.len() < 4 {
        return None;
    }
    let sig = &head[..4];
    // TrueType: 0x00010000 (the "scaler version" 1.0 cast as a u32).
    // True-only Apple variant: `true`. Treat both as TrueType.
    if sig == [0x00, 0x01, 0x00, 0x00] || sig == b"true" {
        return Some(FontFormat::TrueType);
    }
    if sig == b"OTTO" {
        return Some(FontFormat::OpenType);
    }
    if sig == b"ttcf" {
        return Some(FontFormat::Collection);
    }
    if sig == b"wOFF" {
        return Some(FontFormat::Woff);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_from_ext_canonical() {
        assert_eq!(format_from_ext("ttf"), Some(FontFormat::TrueType));
        assert_eq!(format_from_ext("otf"), Some(FontFormat::OpenType));
        assert_eq!(format_from_ext("ttc"), Some(FontFormat::Collection));
        assert_eq!(format_from_ext("otc"), Some(FontFormat::Collection));
        assert_eq!(format_from_ext("woff"), Some(FontFormat::Woff));
        assert_eq!(format_from_ext("txt"), None);
    }

    #[test]
    fn sniff_recognises_each_wrapper() {
        assert_eq!(
            sniff_font_bytes(&[0x00, 0x01, 0x00, 0x00, 0xFF]),
            Some(FontFormat::TrueType)
        );
        assert_eq!(sniff_font_bytes(b"true...."), Some(FontFormat::TrueType));
        assert_eq!(sniff_font_bytes(b"OTTOxxxx"), Some(FontFormat::OpenType));
        assert_eq!(sniff_font_bytes(b"ttcfxxxx"), Some(FontFormat::Collection));
        assert_eq!(sniff_font_bytes(b"wOFFxxxx"), Some(FontFormat::Woff));
    }

    #[test]
    fn sniff_rejects_short_and_unknown_heads() {
        assert_eq!(sniff_font_bytes(b""), None);
        assert_eq!(sniff_font_bytes(b"OTT"), None);
        assert_eq!(sniff_font_bytes(b"PNG\x0d"), None);
        // WOFF2 (brotli, whole-font transform) is still deferred — sniff
        // should not surface it as Font yet.
        assert_eq!(sniff_font_bytes(b"wOF2xxxx"), None);
    }
}
