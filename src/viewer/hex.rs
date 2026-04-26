use std::io;

use anyhow::Result;
use crossterm::event::{self, Event};

use crate::input::detect::{Detected, FileType};
use crate::input::InputSource;
use crate::output::Output;
use crate::theme::{PeekTheme, PeekThemeName};

use super::Viewer;
use super::modes::{HexMode, Mode};
use super::ui::{
    Action, Outcome, ViewMode, ViewerState, keys, make_peek_theme,
    render_themed_status_line, with_alternate_screen,
};

// ---------------------------------------------------------------------------
// Help keys
// ---------------------------------------------------------------------------

const ACTIONS_STANDALONE: &[(Action, &str)] = &[
    (Action::Quit,              "Quit"),
    (Action::ScrollUp,          "Scroll up"),
    (Action::ScrollDown,        "Scroll down"),
    (Action::PageUp,            "Page up"),
    (Action::PageDown,          "Page down"),
    (Action::Top,               "Jump to top"),
    (Action::Bottom,            "Jump to bottom"),
    (Action::ToggleContentInfo, "Toggle hex / file info"),
    (Action::SwitchInfo,        "File info"),
    (Action::ToggleHelp,        "Toggle help"),
    (Action::CycleTheme,        "Next theme"),
];

const ACTIONS_TOGGLE: &[(Action, &str)] = &[
    (Action::Quit,              "Quit"),
    (Action::SwitchToHex,       "Exit hex mode"),
    (Action::ScrollUp,          "Scroll up"),
    (Action::ScrollDown,        "Scroll down"),
    (Action::PageUp,            "Page up"),
    (Action::PageDown,          "Page down"),
    (Action::Top,               "Jump to top"),
    (Action::Bottom,            "Jump to bottom"),
    (Action::ToggleContentInfo, "Toggle hex / file info"),
    (Action::SwitchInfo,        "File info"),
    (Action::ToggleHelp,        "Toggle help"),
    (Action::CycleTheme,        "Next theme"),
];


// ---------------------------------------------------------------------------
// HexViewer
// ---------------------------------------------------------------------------

pub struct HexViewer {
    theme_name: PeekThemeName,
}

impl HexViewer {
    pub fn new(theme_name: PeekThemeName) -> Self {
        Self { theme_name }
    }

    /// Standalone interactive entry: enters its own alternate screen.
    /// `return_on_x = true` means the `x` key exits hex back to the caller;
    /// `false` means `x` is a no-op (e.g., when hex is the default-for-binary
    /// view with no underlying viewer to return to).
    pub fn view_interactive(
        &self,
        source: &InputSource,
        detected: &Detected,
        start_offset: u64,
        return_on_x: bool,
    ) -> Result<()> {
        with_alternate_screen(|stdout| {
            run_hex_loop(
                stdout,
                source,
                detected,
                self.theme_name,
                start_offset,
                return_on_x,
            )
        })
    }
}

impl Viewer for HexViewer {
    fn render(
        &self,
        source: &InputSource,
        _file_type: &FileType,
        output: &mut Output,
    ) -> Result<()> {
        let bs = source.open_byte_source()?;
        let bpr = pipe_bytes_per_row();
        let theme = make_peek_theme(self.theme_name);
        let len = bs.len();
        let chunk_bytes = bpr * 256; // ~4 KB chunks for typical bpr
        let mut offset: u64 = 0;
        while offset < len {
            let buf = bs.read_range(offset, chunk_bytes)?;
            if buf.is_empty() {
                break;
            }
            for (i, row) in buf.chunks(bpr).enumerate() {
                let row_off = offset + (i * bpr) as u64;
                output.write_line(&format_row(&theme, row_off, row, bpr))?;
            }
            offset += buf.len() as u64;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Interactive event loop (callable from inside an existing alt screen)
// ---------------------------------------------------------------------------

/// Run the hex-viewer event loop. Caller must already have entered the
/// alternate screen + raw mode (e.g., via `with_alternate_screen`).
///
/// Drives a `HexMode` for the Content view; reuses `ViewerState` for the
/// Info/Help views, theme cycling, and key dispatch.
pub(crate) fn run_hex_loop(
    stdout: &mut io::Stdout,
    source: &InputSource,
    detected: &Detected,
    initial_theme: PeekThemeName,
    start_offset: u64,
    return_on_x: bool,
) -> Result<()> {
    let actions: &'static [(Action, &str)] = if return_on_x {
        ACTIONS_TOGGLE
    } else {
        ACTIONS_STANDALONE
    };

    let mut hex_mode = HexMode::new(source, start_offset)?;
    let mut state = ViewerState::new(source, detected, initial_theme, Vec::new(), actions)?;
    state.content_lines = hex_mode.render(&state.render_ctx())?;

    let name = source.name().to_string();
    let draw = |stdout: &mut io::Stdout, state: &ViewerState, hex_mode: &HexMode| -> Result<()> {
        let status = render_status_line(&name, state, hex_mode, return_on_x);
        state.draw(stdout, &status)
    };

    draw(stdout, &state, &hex_mode)?;

    loop {
        match event::read()? {
            Event::Key(key) => {
                let Some(action) = keys::dispatch(key, actions) else {
                    continue;
                };

                // Hex owns Content-mode scrolling (byte-based, not line-based).
                if state.view_mode == ViewMode::Content
                    && hex_mode.owns_scroll()
                    && hex_mode.scroll(action)
                {
                    state.content_lines = hex_mode.render(&state.render_ctx())?;
                    draw(stdout, &state, &hex_mode)?;
                    continue;
                }

                match state.apply(action) {
                    Outcome::Quit => return Ok(()),
                    Outcome::Redraw => draw(stdout, &state, &hex_mode)?,
                    Outcome::RecomputeContent => {
                        // Theme cycle: hex content_lines are themed bytes —
                        // re-render so colors match the new theme.
                        state.content_lines = hex_mode.render(&state.render_ctx())?;
                        draw(stdout, &state, &hex_mode)?;
                    }
                    Outcome::Unhandled => {
                        if action == Action::SwitchToHex && return_on_x {
                            return Ok(());
                        }
                    }
                }
            }
            Event::Resize(_, _) => {
                if hex_mode.rerender_on_resize() {
                    hex_mode.realign_to_terminal();
                    state.content_lines = hex_mode.render(&state.render_ctx())?;
                }
                draw(stdout, &state, &hex_mode)?;
            }
            _ => {}
        }
    }
}

fn render_status_line(
    name: &str,
    state: &ViewerState,
    hex_mode: &HexMode,
    return_on_x: bool,
) -> String {
    let theme = &state.peek_theme;
    let mode_label: &str = if state.view_mode == ViewMode::Content {
        hex_mode.label()
    } else {
        state.view_mode.label()
    };

    let hex_segs: Vec<(String, syntect::highlighting::Color)> =
        if state.view_mode == ViewMode::Content {
            hex_mode.status_segments(theme)
        } else {
            Vec::new()
        };

    let mut segments: Vec<(&str, syntect::highlighting::Color)> = vec![
        (name, theme.accent),
        (mode_label, theme.label),
    ];
    for (s, c) in &hex_segs {
        segments.push((s.as_str(), *c));
    }
    segments.push((state.current_theme.cli_name(), theme.muted));

    let hints: Vec<&str> = if return_on_x {
        vec!["x:exit hex", "h:help", "t:theme", "q:quit"]
    } else {
        vec!["h:help", "t:theme", "q:quit"]
    };
    render_themed_status_line(&segments, &hints, theme)
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

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

/// Bytes-per-row for pipe (non-TTY) output: respects $COLUMNS if set and
/// reasonable, otherwise defaults to 16 (classic `hexdump -C`).
pub(crate) fn pipe_bytes_per_row() -> usize {
    if let Ok(s) = std::env::var("COLUMNS") {
        if let Ok(n) = s.parse::<u16>() {
            if n >= 24 {
                return bytes_per_row(n);
            }
        }
    }
    16
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
    let mut out = String::with_capacity(16 + 4 * bpr);
    // Offset
    out.push_str(&theme.paint(&format!("{:08x}", offset), theme.gutter));
    out.push_str("  ");
    // Hex column
    let half = bpr / 2;
    for i in 0..bpr {
        if i == half {
            out.push(' ');
        }
        if i < bytes.len() {
            let b = bytes[i];
            let color = byte_color(theme, b);
            out.push_str(&theme.paint(&format!("{:02x}", b), color));
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
    out.push_str(&theme.paint("|", theme.label));
    for i in 0..bpr {
        if i < bytes.len() {
            let b = bytes[i];
            if (0x20..=0x7e).contains(&b) {
                out.push_str(&theme.paint(&(b as char).to_string(), theme.value));
            } else {
                out.push_str(&theme.paint(".", theme.muted));
            }
        } else {
            out.push(' ');
        }
    }
    out.push_str(&theme.paint("|", theme.label));
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
    use crate::theme::load_embedded_theme;

    fn test_theme() -> PeekTheme {
        let t = load_embedded_theme(PeekThemeName::IslandsDark.tmtheme_source());
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

    #[test]
    fn pipe_bytes_per_row_uses_columns_env() {
        // SAFETY: tests run single-threaded by default within this module's scope
        // for sequential access to env. Save/restore.
        let prev = std::env::var("COLUMNS").ok();
        // Unset → 16
        unsafe { std::env::remove_var("COLUMNS") };
        assert_eq!(pipe_bytes_per_row(), 16);
        // Set to 132 → 24
        unsafe { std::env::set_var("COLUMNS", "132") };
        assert_eq!(pipe_bytes_per_row(), 24);
        // Bogus → 16
        unsafe { std::env::set_var("COLUMNS", "abc") };
        assert_eq!(pipe_bytes_per_row(), 16);
        // Too narrow → 16
        unsafe { std::env::set_var("COLUMNS", "10") };
        assert_eq!(pipe_bytes_per_row(), 16);
        // Restore
        match prev {
            Some(v) => unsafe { std::env::set_var("COLUMNS", v) },
            None => unsafe { std::env::remove_var("COLUMNS") },
        }
    }
}
