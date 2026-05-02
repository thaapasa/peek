use std::fmt::Write as _;

use crate::theme::PeekTheme;

// ---------------------------------------------------------------------------
// Hex layout helpers
// ---------------------------------------------------------------------------
//
// `modes::HexMode` is the sole hex renderer (interactive viewport-clamped
// rendering and pipe-mode full-file streaming both live there). This file
// hosts the layout primitives and per-row formatter shared with that mode.

/// Compute bytes-per-row for a given terminal width. Formula:
///   row width = 14 + 4*bpr
///     (8 offset + 2 spaces + 3*bpr hex (incl. mid-gap) + 2 spaces + 2 pipes + bpr ascii)
/// We pick the largest multiple of 8 (≥ 8) that fits.
pub(crate) fn bytes_per_row(term_cols: u16) -> usize {
    let cols = term_cols as usize;
    let usable = cols.saturating_sub(14);
    let raw = usable / 4;
    ((raw / 8) * 8).max(8)
}

pub(crate) fn align_down(offset: u64, bpr: usize) -> u64 {
    let bpr = bpr as u64;
    if bpr == 0 {
        return 0;
    }
    (offset / bpr) * bpr
}

/// Maximum valid top offset such that the last screen of content is fully
/// utilized. Always aligned to `bpr`.
pub(crate) fn max_top(len: u64, bpr: usize, rows: usize) -> u64 {
    let bpr_u = bpr as u64;
    if bpr_u == 0 || rows == 0 {
        return 0;
    }
    let visible = bpr_u * rows as u64;
    if len <= visible {
        0
    } else {
        let last_row_off = ((len - 1) / bpr_u) * bpr_u;
        last_row_off.saturating_sub(bpr_u * (rows as u64 - 1))
    }
}

// ---------------------------------------------------------------------------
// Row formatting
// ---------------------------------------------------------------------------

/// Format one hex-dump row: themed offset, hex bytes (with mid-gap), and
/// ASCII column. `bytes` may be shorter than `bpr` for the final row.
pub(crate) fn format_row(theme: &PeekTheme, offset: u64, bytes: &[u8], bpr: usize) -> String {
    // Roughly: 14 visible chars + ~12 ANSI escape bytes per colored span,
    // ~3 spans per byte plus a few framing spans.
    let mut out = String::with_capacity(64 + 40 * bpr);

    // Offset — themed `gutter` color, written digit-by-digit into `out`.
    theme.push_fg(&mut out, theme.gutter);
    let _ = write!(out, "{offset:08x}");
    theme.push_reset(&mut out);
    out.push_str("  ");

    // Hex column
    let half = bpr / 2;
    for i in 0..bpr {
        if i == half {
            out.push(' ');
        }
        if i < bytes.len() {
            let b = bytes[i];
            theme.push_fg(&mut out, byte_color(theme, b));
            let _ = write!(out, "{b:02x}");
            theme.push_reset(&mut out);
        } else {
            out.push_str("  ");
        }
        if i + 1 < bpr {
            out.push(' ');
        }
    }

    // Gap between hex and ASCII
    out.push_str("  ");

    // ASCII column
    theme.paint_into(&mut out, "|", theme.label);
    let mut buf = [0u8; 4];
    for i in 0..bpr {
        if i < bytes.len() {
            let b = bytes[i];
            if (0x20..=0x7e).contains(&b) {
                let s = (b as char).encode_utf8(&mut buf);
                theme.paint_into(&mut out, s, theme.value);
            } else {
                theme.paint_into(&mut out, ".", theme.muted);
            }
        } else {
            out.push(' ');
        }
    }
    theme.paint_into(&mut out, "|", theme.label);
    out
}

fn byte_color(theme: &PeekTheme, b: u8) -> syntect::highlighting::Color {
    if (0x20..=0x7e).contains(&b) {
        theme.value
    } else if b == 0x00 || b == 0xff {
        theme.muted
    } else {
        theme.accent
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{PeekThemeName, load_embedded_theme};

    fn test_theme() -> PeekTheme {
        let t = load_embedded_theme(PeekThemeName::IdeaDark.tmtheme_source());
        PeekTheme::from_syntect(&t)
    }

    /// Strip ANSI escape sequences and return the visible text.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut in_escape = false;
        for c in s.chars() {
            if in_escape {
                if c.is_ascii_alphabetic() {
                    in_escape = false;
                }
            } else if c == '\x1b' {
                in_escape = true;
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn bytes_per_row_picks_multiple_of_8() {
        // term_cols = 14 + 4*bpr
        // 80: usable=66, raw=16, → 16
        assert_eq!(bytes_per_row(80), 16);
        // 100: usable=86, raw=21, → 16
        assert_eq!(bytes_per_row(100), 16);
        // 132: usable=118, raw=29, → 24
        assert_eq!(bytes_per_row(132), 24);
        // 200: usable=186, raw=46, → 40
        assert_eq!(bytes_per_row(200), 40);
        // 24: usable=10, raw=2, → floor 8
        assert_eq!(bytes_per_row(24), 8);
        // 40: usable=26, raw=6, → floor 8
        assert_eq!(bytes_per_row(40), 8);
        // very narrow
        assert_eq!(bytes_per_row(0), 8);
    }

    #[test]
    fn format_row_matches_hexdump_c_first_two_rows() {
        let theme = test_theme();
        let bytes_0_15: Vec<u8> = (0u8..=15).collect();
        let bytes_16_31: Vec<u8> = (16u8..=31).collect();
        let row1 = strip_ansi(&format_row(&theme, 0, &bytes_0_15, 16));
        let row2 = strip_ansi(&format_row(&theme, 16, &bytes_16_31, 16));
        assert_eq!(
            row1,
            "00000000  00 01 02 03 04 05 06 07  08 09 0a 0b 0c 0d 0e 0f  |................|"
        );
        assert_eq!(
            row2,
            "00000010  10 11 12 13 14 15 16 17  18 19 1a 1b 1c 1d 1e 1f  |................|"
        );
    }

    #[test]
    fn format_row_renders_printable_ascii() {
        let theme = test_theme();
        let bytes = b"Hello, World!!!\n".to_vec();
        let row = strip_ansi(&format_row(&theme, 0, &bytes, 16));
        // ASCII column should show "Hello, World!!!" then '.' for the newline
        assert!(row.ends_with("|Hello, World!!!.|"));
    }

    #[test]
    fn format_row_handles_short_final_row() {
        let theme = test_theme();
        let row = strip_ansi(&format_row(&theme, 0x1000, b"abcde", 16));
        // 5 bytes followed by 11 byte-slots of "  " (and spacing).
        assert!(row.starts_with("00001000  61 62 63 64 65 "));
        // ASCII column has 5 chars then 11 spaces
        assert!(row.ends_with("|abcde           |"));
    }

    #[test]
    fn format_row_width_matches_formula() {
        let theme = test_theme();
        for &bpr in &[8usize, 16, 24, 32, 40] {
            let bytes: Vec<u8> = (0..bpr as u8).collect();
            let row = strip_ansi(&format_row(&theme, 0, &bytes, bpr));
            assert_eq!(row.len(), 14 + 4 * bpr, "width mismatch for bpr={}", bpr);
        }
    }

    #[test]
    fn max_top_aligns_to_bpr() {
        // file fits on one screen
        assert_eq!(max_top(100, 16, 24), 0);
        // exact fit
        assert_eq!(max_top(16 * 24, 16, 24), 0);
        // one row past exact fit
        assert_eq!(max_top(16 * 24 + 1, 16, 24), 16);
        // file size = 1000, bpr=16, rows=10 → screen=160, last_row_off=992
        // → max_top = 992 - 16*9 = 992-144 = 848
        assert_eq!(max_top(1000, 16, 10), 848);
    }

    #[test]
    fn align_down_works() {
        assert_eq!(align_down(0, 16), 0);
        assert_eq!(align_down(15, 16), 0);
        assert_eq!(align_down(16, 16), 16);
        assert_eq!(align_down(31, 16), 16);
        assert_eq!(align_down(32, 16), 32);
    }
}
