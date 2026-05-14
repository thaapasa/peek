//! Text-search primitives shared by the interactive search feature.
//!
//! Pure pieces, no per-mode state:
//! - [`smart_case_sensitive`] resolves smart-case from a query string.
//! - [`find_matches`] locates non-overlapping query occurrences in a line.
//! - [`overlay_matches`] paints match backgrounds onto an already
//!   ANSI-styled line.
//! - [`SearchState`] holds the result of a scan — every match plus the
//!   `n`/`p` cursor — and the helpers a mode needs to render and
//!   navigate it. Shared by every searchable view.
//!
//! Match positions are byte offsets into *raw* (un-styled) line text;
//! `overlay_matches` is the one place that bridges raw offsets to the
//! escape-interleaved styled string.

use std::ops::Range;

use syntect::highlighting::Color;

use crate::theme::{ActiveStyle, PeekTheme, Sgr, scan};

/// Hard cap on collected search matches. A pathological query (a single
/// common letter in a huge file) would otherwise build an unbounded
/// `Vec`; past the cap the scan stops and the count reflects the cap.
pub(crate) const MAX_MATCHES: usize = 100_000;

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

/// Strip every SGR escape from `s`, leaving only the visible text.
fn strip_ansi(s: &str) -> String {
    scan(s)
        .filter_map(|t| match t {
            Sgr::Text(text) => Some(text),
            Sgr::Esc(_) => None,
        })
        .collect()
}

/// One search match: a byte range within the visible text of logical
/// line `line`.
struct MatchPos {
    line: usize,
    range: Range<usize>,
}

/// The result of a search scan: every match (flat, ordered by
/// `(line, range.start)`) plus the index `n`/`p` cycle through. Shared
/// by every searchable mode — each scans its own lines into one of
/// these, then leans on the methods here to render and navigate.
pub(crate) struct SearchState {
    matches: Vec<MatchPos>,
    current: usize,
}

impl SearchState {
    /// Scan the visible text of `lines` for `query`. Each line's SGR
    /// escapes are stripped before matching, so the returned ranges are
    /// byte offsets into visible text — exactly what [`overlay_matches`]
    /// expects. Smart-case is resolved from the query; the scan stops at
    /// [`MAX_MATCHES`]. `lines` is anything string-like (`&str`,
    /// `String`, `&String`) so streamed and cached sources both fit.
    pub(crate) fn scan<S: AsRef<str>>(lines: impl Iterator<Item = S>, query: &str) -> SearchState {
        let sensitive = smart_case_sensitive(query);
        let mut matches = Vec::new();
        'scan: for (idx, line) in lines.enumerate() {
            let visible = strip_ansi(line.as_ref());
            for range in find_matches(&visible, query, sensitive) {
                matches.push(MatchPos { line: idx, range });
                if matches.len() >= MAX_MATCHES {
                    break 'scan;
                }
            }
        }
        SearchState {
            matches,
            current: 0,
        }
    }

    /// Total match count.
    #[cfg(test)]
    pub(crate) fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Line of the first match, or `None` when the query found nothing.
    pub(crate) fn first_line(&self) -> Option<usize> {
        self.matches.first().map(|m| m.line)
    }

    /// Move the `n`/`p` cursor by `delta` (wrapping at both ends) and
    /// return the new current match's line. `None` when there are no
    /// matches.
    pub(crate) fn step(&mut self, delta: isize) -> Option<usize> {
        let n = self.matches.len();
        if n == 0 {
            return None;
        }
        self.current = (self.current as isize + delta).rem_euclid(n as isize) as usize;
        Some(self.matches[self.current].line)
    }

    /// Match ranges on logical line `line`, plus the local index (into
    /// the returned `Vec`) of the current match when it falls on this
    /// line. `None` when the line has no matches.
    pub(crate) fn line_overlay(&self, line: usize) -> Option<(Vec<Range<usize>>, Option<usize>)> {
        let lo = self.matches.partition_point(|m| m.line < line);
        let hi = self.matches.partition_point(|m| m.line <= line);
        if lo == hi {
            return None;
        }
        let ranges = self.matches[lo..hi]
            .iter()
            .map(|m| m.range.clone())
            .collect();
        // `.then` (lazy) not `.then_some` — `current - lo` underflows
        // for any line rendered after the current match's line.
        let current = (lo..hi).contains(&self.current).then(|| self.current - lo);
        Some((ranges, current))
    }

    /// Status-line segment for the active search: `cur/total` in the
    /// muted colour, or `no match` in the warning colour.
    pub(crate) fn status_segment(&self, theme: &PeekTheme) -> (String, Color) {
        if self.matches.is_empty() {
            ("no match".to_string(), theme.warning)
        } else {
            (
                format!("{}/{}", self.current + 1, self.matches.len()),
                theme.muted,
            )
        }
    }
}

/// Paint search-match backgrounds onto a freshly-sliced viewport. `win`
/// is the visible lines, `scroll` their offset into the full line list
/// (so logical line indices line up with the scan). No-op when no
/// search is active.
pub(crate) fn overlay_window(
    win: &mut [String],
    scroll: usize,
    search: Option<&SearchState>,
    theme: &PeekTheme,
) {
    let Some(search) = search else {
        return;
    };
    for (offset, line) in win.iter_mut().enumerate() {
        if let Some((ranges, current)) = search.line_overlay(scroll + offset) {
            *line = overlay_matches(line, &ranges, current, theme);
        }
    }
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

    #[test]
    fn search_state_scan_finds_and_steps() {
        let lines = ["alpha", "beta hit", "hit again", "delta"];
        let mut s = SearchState::scan(lines.iter(), "hit");
        assert_eq!(s.match_count(), 2);
        assert_eq!(s.first_line(), Some(1));
        // step forward, wrap, step back.
        assert_eq!(s.step(1), Some(2));
        assert_eq!(s.step(1), Some(1));
        assert_eq!(s.step(-1), Some(2));
    }

    #[test]
    fn search_state_strips_ansi_before_matching() {
        // A styled line: the escape bytes must not shift the match
        // offset, and must not themselves be searchable.
        let styled = "\x1b[31mfn\x1b[0m main";
        let s = SearchState::scan(std::iter::once(styled), "main");
        let (ranges, _) = s.line_overlay(0).expect("match on line 0");
        // "fn main" — "main" starts at visible byte 3.
        assert_eq!(ranges, vec![3..7]);
    }

    #[test]
    fn search_state_line_overlay_marks_current() {
        let lines = ["x x", "x"];
        let mut s = SearchState::scan(lines.iter(), "x");
        // matches: line0@0, line0@2, line1@0. current = 0.
        let (ranges, current) = s.line_overlay(0).unwrap();
        assert_eq!(ranges, vec![0..1, 2..3]);
        assert_eq!(current, Some(0));
        // Advance to the third match (line 1); line 0 no longer current.
        s.step(1);
        s.step(1);
        assert_eq!(s.line_overlay(0).unwrap().1, None);
        assert_eq!(s.line_overlay(1).unwrap().1, Some(0));
    }

    #[test]
    fn search_state_status_segment() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let hit = SearchState::scan(["a hit"].iter(), "hit");
        assert_eq!(hit.status_segment(&theme).0, "1/1");
        let miss = SearchState::scan(["abc"].iter(), "zzz");
        assert_eq!(miss.status_segment(&theme).0, "no match");
    }

    #[test]
    fn overlay_window_paints_only_matched_lines() {
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let s = SearchState::scan(["nope", "yes hit", "nope"].iter(), "hit");
        let mut win = vec!["yes hit".to_string(), "nope".to_string()];
        // Window starts at scroll 1 → win[0] is line 1 (has a match).
        overlay_window(&mut win, 1, Some(&s), &theme);
        assert!(win[0].contains("hit") && win[0].len() > "yes hit".len());
        assert_eq!(win[1], "nope");
    }
}
