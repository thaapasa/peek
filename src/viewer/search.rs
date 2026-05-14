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

use crate::theme::PeekTheme;

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
/// Every range gets `theme.search_match` as a background; the `current`
/// range gets `theme.accent` instead so it stands out. Backgrounds are
/// closed with `reset_bg` (`\x1b[49m`), which leaves foreground and
/// attributes untouched. Returns `styled` unchanged when `ranges` is
/// empty.
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
    let match_bg = sm.bg_seq(theme.search_match);
    let current_bg = sm.bg_seq(theme.accent);
    let bg_off = sm.reset_bg();

    let mut out = String::with_capacity(styled.len() + ranges.len() * 24);
    let bytes = styled.as_bytes();
    let mut i = 0;
    let mut raw_pos = 0usize;
    let mut open: Option<usize> = None;
    let mut next = 0usize;

    // Open / close background spans at every boundary the raw cursor has
    // reached. Looped because adjacent ranges close one and open the
    // next at the same `raw_pos`.
    macro_rules! sync_boundaries {
        () => {
            loop {
                if let Some(k) = open {
                    if raw_pos == ranges[k].end {
                        out.push_str(bg_off);
                        open = None;
                        next = k + 1;
                        continue;
                    }
                } else if next < ranges.len() && raw_pos == ranges[next].start {
                    out.push_str(if current == Some(next) {
                        &current_bg
                    } else {
                        &match_bg
                    });
                    open = Some(next);
                    continue;
                }
                break;
            }
        };
    }

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // Copy the SGR escape verbatim — it doesn't advance the raw
            // cursor and must not be split by a background span.
            let start = i;
            i += 1;
            while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            out.push_str(&styled[start..i]);
            continue;
        }
        sync_boundaries!();
        let ch = styled[i..].chars().next().expect("char boundary");
        out.push(ch);
        i += ch.len_utf8();
        raw_pos += ch.len_utf8();
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

    #[test]
    fn overlay_plain_line_wraps_match() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let bg = theme.style_mode.bg_seq(theme.search_match);
        let off = theme.style_mode.reset_bg();
        // "abcde" with match 1..3 → a {bg} bc {off} de
        assert_eq!(
            overlay_matches("abcde", &[1..3], None, &theme),
            format!("a{bg}bc{off}de")
        );
    }

    #[test]
    fn overlay_current_uses_accent_bg() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let match_bg = theme.style_mode.bg_seq(theme.search_match);
        let cur_bg = theme.style_mode.bg_seq(theme.accent);
        let off = theme.style_mode.reset_bg();
        // Two matches; index 1 is current.
        assert_eq!(
            overlay_matches("x1x2x", &[1..2, 3..4], Some(1), &theme),
            format!("x{match_bg}1{off}x{cur_bg}2{off}x")
        );
    }

    #[test]
    fn overlay_skips_escape_sequences_for_raw_offset() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let bg = theme.style_mode.bg_seq(theme.search_match);
        let off = theme.style_mode.reset_bg();
        // Styled "ab" where 'a' carries a fg escape; raw text is "ab",
        // match 1..2 must land on 'b' regardless of the escape bytes.
        let fg = theme.style_mode.fg_seq(theme.foreground);
        let styled = format!("{fg}ab\x1b[0m");
        let got = overlay_matches(&styled, &[1..2], None, &theme);
        // The match runs to end-of-line: the line's own `\x1b[0m` is
        // copied during the walk, then the span is flushed with `off`.
        assert_eq!(got, format!("{fg}a{bg}b\x1b[0m{off}"));
    }

    #[test]
    fn overlay_match_to_end_of_line() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let bg = theme.style_mode.bg_seq(theme.search_match);
        let off = theme.style_mode.reset_bg();
        assert_eq!(
            overlay_matches("abc", &[1..3], None, &theme),
            format!("a{bg}bc{off}")
        );
    }
}
