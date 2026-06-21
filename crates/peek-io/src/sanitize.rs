//! Neutralise terminal-control codepoints in untrusted text before it is
//! displayed.
//!
//! peek renders file content and file-derived names (status bar, listings)
//! to a terminal, wrapping them in its *own* SGR colour escapes. Those two
//! streams are indistinguishable once mixed — the styled-string scanner
//! (`peek_theme::scan`) classifies any `ESC` as an SGR token — so a raw
//! `ESC`/OSC/control byte carried in *content* would be relayed verbatim
//! to the terminal. That is the classic file-viewer hole: OSC 52 clipboard
//! writes, OSC 0 title spoofing, OSC 8 hyperlink injection, and C0 cursor
//! moves, all triggered by merely viewing a hostile file (or a file with a
//! hostile name).
//!
//! The fix is to strip control sequences at the point text is *produced*
//! from untrusted bytes — before peek adds any escapes of its own. This is
//! the canonical helper every such producer routes through (raw line
//! decode, pretty-print output, status/listing labels).

use std::borrow::Cow;

/// Map terminal-control codepoints to width-stable visible glyphs.
///
/// Substitution is **1-for-1** (one `char` in, one `char` out), so the
/// codepoint count is preserved — wrap geometry and line anchors computed
/// over the sanitized text stay aligned. Note this does *not* preserve byte
/// offsets (`ESC` → `␛` grows 1→3 bytes): the byte-offset search path stays
/// aligned only because it scans and paints the *same* sanitized string
/// (sanitized-vs-sanitized), not because byte positions are stable.
///
/// Preserved: TAB (`0x09`) and LF (`0x0A`), the line structure the render
/// layer owns (TAB via `expand_tabs`, LF via the line splitter). Replaced:
/// the rest of C0 *including CR and ESC*, DEL (`0x7F`), the C1 range
/// (`0x80..=0x9F`), and the bidirectional override / isolate controls
/// (`U+202A..=U+202E`, `U+2066..=U+2069`) that can visually reorder text
/// for spoofing. C0/DEL become their Unicode Control-Picture glyph
/// (`ESC` → `␛`); C1 and bidi controls become `U+FFFD`.
///
/// Returns `Cow::Borrowed` when the input is already clean — the common
/// case — so nothing allocates.
pub fn sanitize_terminal_controls(s: &str) -> Cow<'_, str> {
    if !s.chars().any(needs_sanitizing) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        out.push(if needs_sanitizing(c) {
            replacement(c)
        } else {
            c
        });
    }
    Cow::Owned(out)
}

fn needs_sanitizing(c: char) -> bool {
    match c {
        '\t' | '\n' => false,
        '\0'..='\u{1f}' | '\u{7f}' => true, // C0 (minus TAB/LF) + DEL
        '\u{80}'..='\u{9f}' => true,        // C1
        '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => true, // bidi overrides/isolates
        _ => false,
    }
}

fn replacement(c: char) -> char {
    match c {
        // Control Pictures (U+2400..) are the visible symbols for C0 codes.
        '\0'..='\u{1f}' => char::from_u32(0x2400 + c as u32).unwrap_or('\u{fffd}'),
        '\u{7f}' => '\u{2421}', // ␡ SYMBOL FOR DELETE
        _ => '\u{fffd}',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_is_borrowed() {
        assert!(matches!(
            sanitize_terminal_controls("hello world\tindented"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn esc_and_osc_are_neutralised() {
        // OSC 52 clipboard write + OSC 0 title set — the headline attack.
        let evil = "x\x1b]52;c;cHduZWQ=\x07 \x1b]0;PWNED\x07";
        let out = sanitize_terminal_controls(evil);
        assert!(!out.contains('\x1b'), "ESC must not survive: {out:?}");
        assert!(!out.contains('\x07'), "BEL must not survive: {out:?}");
        assert!(out.contains('\u{241b}'), "ESC → ␛ picture: {out:?}");
    }

    #[test]
    fn tab_and_newline_preserved() {
        let s = "a\tb\nc";
        assert_eq!(sanitize_terminal_controls(s).as_ref(), s);
    }

    #[test]
    fn cr_is_replaced() {
        // A lone CR returns the cursor to column 0, overwriting the line.
        assert_eq!(sanitize_terminal_controls("a\rb").as_ref(), "a\u{240d}b");
    }

    #[test]
    fn offsets_are_stable_one_for_one() {
        let s = "\x1b[31mred\x1b[0m";
        let out = sanitize_terminal_controls(s);
        assert_eq!(out.chars().count(), s.chars().count());
    }

    #[test]
    fn c1_and_bidi_become_replacement() {
        assert_eq!(
            sanitize_terminal_controls("a\u{202e}b").as_ref(),
            "a\u{fffd}b"
        );
        assert_eq!(
            sanitize_terminal_controls("a\u{85}b").as_ref(),
            "a\u{fffd}b"
        );
    }
}
