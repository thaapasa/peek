use std::io;

use anyhow::Result;
use crossterm::{cursor, execute, terminal};
use syntect::highlighting::Color;
use unicode_width::UnicodeWidthChar;

use crate::theme::{ColorMode, PeekTheme, PeekThemeName, load_embedded_theme};

pub(crate) mod help;
pub(crate) mod keys;
pub(crate) mod state;

pub(crate) use keys::{Action, Outcome};
pub(crate) use state::{GLOBAL_ACTIONS, ViewerState};

/// Enter the alternate screen and raw mode, run the closure, then always clean up.
///
/// Cleanup runs via `Drop`, so a panic inside `f` still restores the
/// terminal — without the guard, an unwinding panic would leave the
/// user's shell in raw-mode + alternate-screen, which is unrecoverable
/// without `reset(1)`.
pub(crate) fn with_alternate_screen(f: impl FnOnce(&mut io::Stdout) -> Result<()>) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::MoveTo(0, 0),
        cursor::Hide,
    )?;
    terminal::enable_raw_mode()?;

    let _guard = TerminalGuard;
    f(&mut stdout)
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show, terminal::LeaveAlternateScreen);
    }
}

/// Build a themed status line from labeled segments and hint strings.
///
/// `segments` are shown on the left, joined by muted `│` separators.
/// `hints` are shown on the right, all in the muted color.
/// The whole line gets the theme's `selection` background.
pub(crate) fn render_themed_status_line(
    segments: &[(&str, Color)],
    hints: &[&str],
    theme: &PeekTheme,
) -> String {
    let sep = theme.paint_fg("\u{2502}", theme.muted);

    let left = segments
        .iter()
        .map(|(text, color)| theme.paint_fg(text, *color))
        .collect::<Vec<_>>()
        .join(&format!(" {sep} "));
    let left = format!(" {left}");

    let hints = hints
        .iter()
        .map(|h| theme.paint_fg(h, theme.muted))
        .collect::<Vec<_>>()
        .join("  ");
    let hints = format!("{hints} ");

    let cols = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    theme.paint_bg(&compose_status_line(&left, &hints, cols), theme.selection)
}

/// Compose a status line from left and right parts, padding or truncating to fit `cols`.
/// Drops hints first, then truncates left if still too wide.
fn compose_status_line(left: &str, hints: &str, cols: usize) -> String {
    let left_w = strip_ansi_width(left);
    let hints_w = strip_ansi_width(hints);

    if left_w + hints_w <= cols {
        let gap = cols.saturating_sub(left_w + hints_w);
        format!("{}{}{}", left, " ".repeat(gap), hints)
    } else if left_w < cols {
        let remaining = cols.saturating_sub(left_w);
        let truncated_hints = truncate_ansi(hints, remaining);
        let hints_actual = strip_ansi_width(&truncated_hints);
        let pad = cols.saturating_sub(left_w + hints_actual);
        format!("{}{}{}", left, " ".repeat(pad), truncated_hints)
    } else {
        truncate_ansi(left, cols)
    }
}

/// Count the visible terminal-column width of a string, ignoring ANSI
/// escape sequences. CJK and emoji are treated as 2 cols; combining marks
/// as 0 cols (per `unicode-width`).
pub(crate) fn strip_ansi_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            width += UnicodeWidthChar::width(c).unwrap_or(0);
        }
    }
    width
}

/// Wrap a string containing SGR escape sequences into chunks of at most
/// `width` visible terminal columns. The active foreground style is
/// preserved across chunk boundaries: each non-empty chunk closes with a
/// reset and the next reopens with the most recently seen non-reset SGR
/// (so syntect's per-line colors don't bleed past a cut).
///
/// Wide characters (CJK width 2) are never split — a glyph that would
/// straddle the boundary is pushed to the next chunk in full. Combining
/// marks (width 0) attach to whichever chunk holds their preceding cell.
/// Empty input returns a single empty chunk so callers can map "one
/// logical line" to "at least one visual row".
pub(crate) fn wrap_styled(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut col = 0usize;
    let mut active_seq = String::new();
    let mut esc_buf = String::new();
    let mut in_escape = false;

    for c in s.chars() {
        if in_escape {
            esc_buf.push(c);
            if c.is_ascii_alphabetic() {
                in_escape = false;
                if esc_buf == "\x1b[0m" {
                    active_seq.clear();
                } else {
                    active_seq.clear();
                    active_seq.push_str(&esc_buf);
                }
                cur.push_str(&esc_buf);
                esc_buf.clear();
            }
        } else if c == '\x1b' {
            in_escape = true;
            esc_buf.push(c);
        } else {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if cw > 0 && col + cw > width {
                if !active_seq.is_empty() {
                    cur.push_str("\x1b[0m");
                }
                out.push(std::mem::take(&mut cur));
                col = 0;
                if !active_seq.is_empty() {
                    cur.push_str(&active_seq);
                }
            }
            cur.push(c);
            col += cw;
        }
    }
    out.push(cur);
    out
}

/// Count how many visual chunks `wrap_styled(s, width)` would produce
/// without actually allocating them. Returns at least 1 (an empty line
/// still occupies one visual row). Matches `wrap_styled`'s wide-char
/// semantics: a glyph that would straddle the boundary triggers a wrap.
pub(crate) fn count_wrap_segments(s: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let mut count = 1usize;
    let mut col = 0usize;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if cw > 0 && col + cw > width {
                count += 1;
                col = 0;
            }
            col += cw;
        }
    }
    count
}

/// Slice a string containing SGR escape sequences to a horizontal window:
/// skip the first `start_col` visible cells, return up to `max_cols`
/// cells. Style continuity preserved — the most recent non-reset SGR
/// active at the slice start is re-emitted at the head of the output, and
/// any trailing reset is emitted at the tail.
///
/// Wide characters straddling either boundary are dropped rather than
/// split. Combining marks at the start of the emit region attach to no
/// preceding cell and are dropped.
pub(crate) fn slice_styled_h(s: &str, start_col: usize, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut col = 0usize;
    let mut taken = 0usize;
    let mut active_seq = String::new();
    let mut esc_buf = String::new();
    let mut in_escape = false;
    let mut started = false;

    for c in s.chars() {
        if in_escape {
            esc_buf.push(c);
            if c.is_ascii_alphabetic() {
                in_escape = false;
                if esc_buf == "\x1b[0m" {
                    active_seq.clear();
                } else {
                    active_seq.clear();
                    active_seq.push_str(&esc_buf);
                }
                if started {
                    out.push_str(&esc_buf);
                }
                esc_buf.clear();
            }
        } else if c == '\x1b' {
            in_escape = true;
            esc_buf.push(c);
        } else {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if col < start_col {
                col += cw;
                continue;
            }
            if !started {
                if !active_seq.is_empty() {
                    out.push_str(&active_seq);
                }
                started = true;
            }
            if cw > 0 && taken + cw > max_cols {
                break;
            }
            out.push(c);
            taken += cw;
            col += cw;
        }
    }
    if started && !active_seq.is_empty() {
        out.push_str("\x1b[0m");
    }
    out
}

/// Truncate a string containing ANSI escapes to at most `max_width`
/// visible terminal columns. A wide character (e.g. CJK) that wouldn't
/// fit completely is dropped rather than split.
pub(crate) fn truncate_ansi(s: &str, max_width: usize) -> String {
    let mut result = String::with_capacity(s.len());
    let mut width = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            result.push(c);
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
            result.push(c);
        } else {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if width + cw > max_width {
                break;
            }
            result.push(c);
            width += cw;
        }
    }
    result
}

pub(crate) fn make_peek_theme(name: PeekThemeName, color_mode: ColorMode) -> PeekTheme {
    let syntect_theme = load_embedded_theme(name.tmtheme_source());
    let mut t = PeekTheme::from_syntect(&syntect_theme);
    t.color_mode = color_mode;
    t
}

pub(crate) fn terminal_rows() -> usize {
    terminal::size().map(|(_, h)| h as usize).unwrap_or(24)
}

pub(crate) fn terminal_cols() -> usize {
    terminal::size().map(|(w, _)| w as usize).unwrap_or(80)
}

/// Visible rows available for content (total rows minus status line).
pub(crate) fn content_rows() -> usize {
    terminal_rows().saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_width_skips_escape_sequences() {
        assert_eq!(strip_ansi_width("hello"), 5);
        assert_eq!(strip_ansi_width("\x1b[31mhello\x1b[0m"), 5);
        assert_eq!(strip_ansi_width(""), 0);
    }

    #[test]
    fn strip_ansi_width_counts_cjk_as_two_cols() {
        assert_eq!(strip_ansi_width("你好"), 4);
        assert_eq!(strip_ansi_width("a你b"), 4);
    }

    #[test]
    fn truncate_ansi_caps_visible_width() {
        assert_eq!(truncate_ansi("abcdef", 3), "abc");
        assert_eq!(truncate_ansi("abcdef", 6), "abcdef");
        assert_eq!(truncate_ansi("abcdef", 10), "abcdef");
    }

    #[test]
    fn truncate_ansi_drops_wide_char_that_would_split() {
        // CJK char has width 2 — at max_width 1 it can't fit at all.
        assert_eq!(truncate_ansi("你好", 1), "");
        // At max_width 3 only the first CJK fits (width 2); the second
        // would push width to 4 > 3 so it's dropped whole rather than split.
        assert_eq!(truncate_ansi("你好", 3), "你");
    }

    #[test]
    fn truncate_ansi_preserves_trailing_escape() {
        // Visible content fits exactly; the trailing reset escape — having
        // zero visible width — is still emitted.
        assert_eq!(truncate_ansi("hi\x1b[0m", 2), "hi\x1b[0m");
    }

    #[test]
    fn status_line_fits_pads_between_left_and_hints() {
        let s = compose_status_line("left", "hints", 20);
        assert_eq!(strip_ansi_width(&s), 20);
        assert_eq!(s, "left           hints");
    }

    #[test]
    fn status_line_truncates_hints_when_room_is_tight() {
        // left fits (4 < 10) but hints (10) don't — hints get truncated to
        // the remaining 6 cols.
        let s = compose_status_line("left", "0123456789", 10);
        assert_eq!(strip_ansi_width(&s), 10);
        assert_eq!(s, "left012345");
    }

    #[test]
    fn status_line_truncates_left_when_no_room_for_hints() {
        // left alone is wider than cols — drop hints entirely and clip left.
        let s = compose_status_line("0123456789", "hints", 5);
        assert_eq!(strip_ansi_width(&s), 5);
        assert_eq!(s, "01234");
    }

    #[test]
    fn wrap_styled_plain_ascii_chunks_by_width() {
        let chunks = wrap_styled("abcdefghij", 3);
        assert_eq!(chunks, vec!["abc", "def", "ghi", "j"]);
    }

    #[test]
    fn wrap_styled_empty_input_yields_one_empty_chunk() {
        // An empty logical line should still consume one visual row.
        assert_eq!(wrap_styled("", 5), vec![""]);
    }

    #[test]
    fn wrap_styled_short_line_single_chunk() {
        assert_eq!(wrap_styled("hi", 80), vec!["hi"]);
    }

    #[test]
    fn wrap_styled_preserves_active_style_across_cuts() {
        // `RED` set, three chars, `RESET`. Width 2 → cut between b and c.
        // First chunk: RED + "ab" + RESET. Second chunk: RED + "c" + RESET.
        let red = "\x1b[31m";
        let reset = "\x1b[0m";
        let input = format!("{red}abc{reset}");
        let chunks = wrap_styled(&input, 2);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], format!("{red}ab{reset}"));
        // Second chunk re-emits the active fg, then "c", then the
        // explicit reset that was in the source stream.
        assert_eq!(chunks[1], format!("{red}c{reset}"));
    }

    #[test]
    fn wrap_styled_drops_wide_char_at_boundary() {
        // ASCII "ab" then CJK "你" (width 2) then ASCII "c". Width 3:
        // chunk 0 holds "ab" (col=2); the CJK would push col to 4 > 3,
        // so wrap and start chunk 1 with "你", then "c" still fits at col=3.
        let chunks = wrap_styled("ab你c", 3);
        assert_eq!(chunks, vec!["ab", "你c"]);
    }

    #[test]
    fn wrap_styled_combining_mark_attaches_to_previous() {
        // `e` + combining acute (U+0301, width 0). Should be one chunk.
        assert_eq!(wrap_styled("e\u{0301}", 1), vec!["e\u{0301}"]);
    }

    #[test]
    fn slice_styled_h_plain_window() {
        assert_eq!(slice_styled_h("0123456789", 3, 4), "3456");
    }

    #[test]
    fn slice_styled_h_zero_max_cols_empty() {
        assert_eq!(slice_styled_h("abc", 0, 0), "");
    }

    #[test]
    fn slice_styled_h_start_past_end_empty() {
        assert_eq!(slice_styled_h("abc", 10, 4), "");
    }

    #[test]
    fn slice_styled_h_reemits_active_style_at_window_start() {
        let red = "\x1b[31m";
        let reset = "\x1b[0m";
        let input = format!("{red}abcdef{reset}");
        // Skip first 2 cells, take 3.
        let out = slice_styled_h(&input, 2, 3);
        assert_eq!(out, format!("{red}cde{reset}"));
    }

    #[test]
    fn slice_styled_h_wide_char_straddling_start_dropped() {
        // start_col=1, "你bc" — CJK at col 0 has width 2, straddles 0..1
        // so it's dropped; emit starts at "b". max_cols=2 → "bc".
        assert_eq!(slice_styled_h("你bc", 1, 2), "bc");
    }

    #[test]
    fn slice_styled_h_wide_char_straddling_end_dropped() {
        // "ab你c" with start=0, max=3: a(1) b(2) 你 would push to 4>3 → drop.
        assert_eq!(slice_styled_h("ab你c", 0, 3), "ab");
    }

    #[test]
    fn status_line_handles_cjk_widths() {
        // "你好" has visible width 4; padding accounts for that, not byte len.
        let s = compose_status_line("你好", "你好", 10);
        assert_eq!(strip_ansi_width(&s), 10);
        assert!(s.starts_with("你好"));
        assert!(s.ends_with("你好"));
    }
}
