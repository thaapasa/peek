//! Status-line composition: themed left segments + muted right hints,
//! padded or truncated to the terminal width (hints drop first, then the
//! left side clips).

use crossterm::terminal;
use syntect::highlighting::Color;

use peek_theme::PeekTheme;

use super::styled::{strip_ansi_width, truncate_ansi};

/// Build a themed status line from labeled segments and hint strings.
///
/// `segments` are shown on the left, joined by muted `│` separators.
/// `hints` are shown on the right, all in the muted color.
/// The whole line gets the theme's `selection` background.
pub fn render_themed_status_line(
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn status_line_handles_cjk_widths() {
        // "你好" has visible width 4; padding accounts for that, not byte len.
        let s = compose_status_line("你好", "你好", 10);
        assert_eq!(strip_ansi_width(&s), 10);
        assert!(s.starts_with("你好"));
        assert!(s.ends_with("你好"));
    }
}
