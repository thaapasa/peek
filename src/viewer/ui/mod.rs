use std::io;

use anyhow::Result;
use crossterm::{cursor, execute, terminal};
use syntect::highlighting::Color;

use crate::theme::{PeekTheme, PeekThemeName, load_embedded_theme};

pub(crate) mod help;
pub(crate) mod keys;
pub(crate) mod state;

pub(crate) use keys::{Action, Outcome};
pub(crate) use state::{GLOBAL_ACTIONS, ViewerState};

/// Enter the alternate screen and raw mode, run the closure, then always clean up.
pub(crate) fn with_alternate_screen(
    f: impl FnOnce(&mut io::Stdout) -> Result<()>,
) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::MoveTo(0, 0),
        cursor::Hide,
    )?;
    terminal::enable_raw_mode()?;

    let result = f(&mut stdout);

    // Always clean up, even on error
    let _ = terminal::disable_raw_mode();
    let _ = execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen);

    result
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

/// Count the visible character width of a string, ignoring ANSI escape sequences.
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
            width += 1;
        }
    }
    width
}

/// Truncate a string containing ANSI escapes to at most `max_width` visible characters.
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
            if width >= max_width {
                break;
            }
            result.push(c);
            width += 1;
        }
    }
    result
}

pub(crate) fn make_peek_theme(name: PeekThemeName) -> PeekTheme {
    let syntect_theme = load_embedded_theme(name.tmtheme_source());
    PeekTheme::from_syntect(&syntect_theme)
}

pub(crate) fn terminal_rows() -> usize {
    terminal::size().map(|(_, h)| h as usize).unwrap_or(24)
}

/// Visible rows available for content (total rows minus status line).
pub(crate) fn content_rows() -> usize {
    terminal_rows().saturating_sub(1)
}
