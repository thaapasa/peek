//! Text-search primitives shared by the interactive search feature.
//!
//! Three pure pieces, no viewer state:
//! - [`smart_case_sensitive`] resolves smart-case from a query string.
//! - [`find_matches`] locates non-overlapping query occurrences in a line.
//! - [`overlay_matches`] paints match backgrounds onto an already
//!   ANSI-styled line.
//!
//! Match positions are byte offsets into *raw* (un-styled) line text;
//! `overlay_matches` is the one place that bridges raw offsets to the
//! escape-interleaved styled string.

use std::ops::Range;

use crate::theme::{ActiveStyle, PeekTheme, Sgr, scan};

/// Smart-case rule: a query with any uppercase character searches
/// case-sensitively; an all-lowercase query searches case-insensitively.
pub(crate) fn smart_case_sensitive(query: &str) -> bool {
    query.chars().any(|c| c.is_uppercase())
}

/// Locate every non-overlapping occurrence of `query` in `haystack`,
/// left-to-right, as byte ranges into `haystack`.
///
/// Case-insensitive matching folds ASCII letters only and compares
/// byte-for-byte, so the returned ranges are always exact byte spans of
/// `haystack` (folding never changes UTF-8 length). An empty query, or a
/// query longer than the haystack, yields no matches.
pub(crate) fn find_matches(haystack: &str, query: &str, sensitive: bool) -> Vec<Range<usize>> {
    if query.is_empty() || query.len() > haystack.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if sensitive {
        let mut start = 0;
        while let Some(rel) = haystack[start..].find(query) {
            let at = start + rel;
            out.push(at..at + query.len());
            start = at + query.len();
        }
    } else {
        let hb = haystack.as_bytes();
        let qb = query.as_bytes();
        let qlen = qb.len();
        let mut i = 0;
        while i + qlen <= hb.len() {
            if haystack.is_char_boundary(i)
                && hb[i..i + qlen]
                    .iter()
                    .zip(qb)
                    .all(|(x, y)| x.eq_ignore_ascii_case(y))
            {
                out.push(i..i + qlen);
                i += qlen;
            } else {
                i += 1;
            }
        }
    }
    out
}

/// Paint match backgrounds onto an already-styled line.
///
/// `styled` is a line with embedded SGR escapes (syntect output, or a
/// plain copy). `ranges` are byte offsets into the line's *raw visible
/// text* — the same offsets [`find_matches`] returns — and must be
/// sorted and non-overlapping. `current` is the index *within `ranges`*
/// of the active match (the one `n`/`p` landed on), or `None` when the
/// active match is on another line.
///
/// A match gets an explicit background **and** foreground pair — the
/// syntax colour underneath is dropped so matched text looks uniform
/// regardless of what it was (an XML tag and the text inside it
/// highlight the same). Inside a span the styled line's own foreground
/// escapes are suppressed and tracked; when the span closes the syntax
/// colour is restored so the rest of the line is unaffected.
///
/// Both states' colours come from the theme's `accent` hue — the
/// current match vivid (`search_current_style`), the rest muted
/// (`search_match_style`) — each paired with a neutral contrasting
/// foreground. Returns `styled` unchanged when `ranges` is empty.
pub(crate) fn overlay_matches(
    styled: &str,
    ranges: &[Range<usize>],
    current: Option<usize>,
    theme: &PeekTheme,
) -> String {
    if ranges.is_empty() {
        return styled.to_string();
    }
    let sm = theme.style_mode;
    let (match_bg_c, match_fg_c) = theme.search_match_style();
    let (current_bg_c, current_fg_c) = theme.search_current_style();
    let match_bg = sm.bg_seq(match_bg_c);
    let match_fg = sm.fg_seq(match_fg_c);
    let current_bg = sm.bg_seq(current_bg_c);
    let current_fg = sm.fg_seq(current_fg_c);
    let bg_off = sm.reset_bg();
    let fg_off = sm.reset_fg();

    let mut out = String::with_capacity(styled.len() + ranges.len() * 32);
    let mut raw_pos = 0usize;
    let mut open: Option<usize> = None;
    let mut next = 0usize;
    // Tracks the styled line's own foreground so it can be restored when
    // a match span closes (only `.fg()` is read — the input is
    // foreground-only syntect output).
    let mut active = ActiveStyle::default();

    // Open / close match spans at every boundary the raw cursor has
    // reached. Looped because adjacent ranges close one and open the
    // next at the same `raw_pos`.
    macro_rules! sync_boundaries {
        () => {
            loop {
                if let Some(k) = open {
                    if raw_pos == ranges[k].end {
                        out.push_str(bg_off);
                        let fg = active.fg();
                        out.push_str(if fg.is_empty() { fg_off } else { fg });
                        open = None;
                        next = k + 1;
                        continue;
                    }
                } else if next < ranges.len() && raw_pos == ranges[next].start {
                    let (bg, fg) = if current == Some(next) {
                        (&current_bg, &current_fg)
                    } else {
                        (&match_bg, &match_fg)
                    };
                    out.push_str(bg);
                    out.push_str(fg);
                    open = Some(next);
                    continue;
                }
                break;
            }
        };
    }

    for token in scan(styled) {
        match token {
            Sgr::Esc(esc) => {
                // Track the syntax foreground for post-span restore.
                // Inside a span the styled line's own colours are
                // suppressed so the match style stays uniform; outside,
                // copy through.
                active.observe(esc);
                if open.is_none() {
                    out.push_str(esc);
                }
            }
            Sgr::Text(text) => {
                for ch in text.chars() {
                    sync_boundaries!();
                    out.push(ch);
                    raw_pos += ch.len_utf8();
                }
            }
        }
    }
    // Flush a span that runs to end-of-line.
    sync_boundaries!();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{PeekThemeName, StyleMode};
    use crate::viewer::ui::make_peek_theme;

    #[test]
    fn smart_case_detects_uppercase() {
        assert!(!smart_case_sensitive("foo"));
        assert!(!smart_case_sensitive("foo_bar 123"));
        assert!(smart_case_sensitive("Foo"));
        assert!(smart_case_sensitive("fooBAR"));
    }

    #[test]
    fn find_matches_case_sensitive() {
        assert_eq!(find_matches("foo Foo foo", "foo", true), vec![0..3, 8..11]);
        assert_eq!(find_matches("foo Foo foo", "Foo", true), vec![4..7]);
        assert_eq!(find_matches("abc", "xyz", true), Vec::<Range<usize>>::new());
    }

    #[test]
    fn find_matches_case_insensitive() {
        assert_eq!(
            find_matches("foo Foo FOO", "foo", false),
            vec![0..3, 4..7, 8..11]
        );
    }

    #[test]
    fn find_matches_non_overlapping() {
        // "aaaa" with query "aa" → two matches, not three.
        assert_eq!(find_matches("aaaa", "aa", true), vec![0..2, 2..4]);
    }

    #[test]
    fn find_matches_non_ascii_offsets_exact() {
        // "äbäb" — each 'ä' is 2 bytes: ä@0 b@2 ä@3 b@5.
        let h = "äbäb";
        assert_eq!(find_matches(h, "b", false), vec![2..3, 5..6]);
        // Case-insensitive match that includes the non-ASCII char keeps
        // exact byte length.
        assert_eq!(find_matches("xÄy", "ä", false), Vec::<Range<usize>>::new());
        assert_eq!(find_matches("xäy", "ä", false), vec![1..3]);
    }

    #[test]
    fn find_matches_empty_query() {
        assert_eq!(find_matches("abc", "", true), Vec::<Range<usize>>::new());
    }

    #[test]
    fn overlay_empty_ranges_passthrough() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        assert_eq!(overlay_matches("hello", &[], None, &theme), "hello");
    }

    /// Sequences for the non-current and current match styles, plus the
    /// shared resets — keeps the overlay assertions readable.
    fn match_seqs(theme: &PeekTheme) -> (String, String, String, String, String, String) {
        let sm = theme.style_mode;
        let (mbg, mfg) = theme.search_match_style();
        let (cbg, cfg) = theme.search_current_style();
        (
            sm.bg_seq(mbg),
            sm.fg_seq(mfg),
            sm.bg_seq(cbg),
            sm.fg_seq(cfg),
            sm.reset_bg().to_string(),
            sm.reset_fg().to_string(),
        )
    }

    #[test]
    fn overlay_plain_line_wraps_match() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let (mbg, mfg, _, _, bg_off, fg_off) = match_seqs(&theme);
        // "abcde" with match 1..3 → a {mbg}{mfg} bc {bg_off}{fg_off} de.
        // No prior syntax colour, so the span closes back to default fg.
        assert_eq!(
            overlay_matches("abcde", &[1..3], None, &theme),
            format!("a{mbg}{mfg}bc{bg_off}{fg_off}de")
        );
    }

    #[test]
    fn overlay_current_uses_distinct_style() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let (mbg, mfg, cbg, cfg, bg_off, fg_off) = match_seqs(&theme);
        // The current match's background must differ from the rest.
        assert_ne!(mbg, cbg);
        // Two matches; index 1 is current — it gets the vivid style.
        assert_eq!(
            overlay_matches("x1x2x", &[1..2, 3..4], Some(1), &theme),
            format!("x{mbg}{mfg}1{bg_off}{fg_off}x{cbg}{cfg}2{bg_off}{fg_off}x")
        );
    }

    #[test]
    fn overlay_skips_escape_sequences_for_raw_offset() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let (mbg, mfg, _, _, bg_off, fg_off) = match_seqs(&theme);
        // Styled "ab" where 'a' carries a fg escape; raw text is "ab",
        // match 1..2 must land on 'b' regardless of the escape bytes.
        let fg = theme.style_mode.fg_seq(theme.foreground);
        let styled = format!("{fg}ab\x1b[0m");
        let got = overlay_matches(&styled, &[1..2], None, &theme);
        // The trailing `\x1b[0m` falls inside the match span, so it's
        // suppressed; the span closes with bg + fg resets.
        assert_eq!(got, format!("{fg}a{mbg}{mfg}b{bg_off}{fg_off}"));
    }

    #[test]
    fn overlay_restores_syntax_color_after_match() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let (mbg, mfg, _, _, bg_off, _) = match_seqs(&theme);
        // A syntax fg ("red") is active across the match; after the span
        // closes, the syntax colour must resume — not a bare reset.
        let red = "\x1b[31m";
        let styled = format!("{red}abcd\x1b[0m");
        let got = overlay_matches(&styled, &[1..2], None, &theme);
        assert_eq!(got, format!("{red}a{mbg}{mfg}b{bg_off}{red}cd\x1b[0m"));
    }

    #[test]
    fn overlay_match_to_end_of_line() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let (mbg, mfg, _, _, bg_off, fg_off) = match_seqs(&theme);
        assert_eq!(
            overlay_matches("abc", &[1..3], None, &theme),
            format!("a{mbg}{mfg}bc{bg_off}{fg_off}")
        );
    }
}
