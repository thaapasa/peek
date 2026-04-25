use std::io::{self, Write};

use anyhow::Result;
use crossterm::{
    cursor, execute,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal,
};

use crate::detect::FileType;
use crate::input::{ByteSource, InputSource};
use crate::pager::Output;
use crate::theme::{ANSI_RESET_BYTES, PeekTheme, PeekThemeName};

use super::Viewer;
use super::ui::{
    KeyAction, ViewMode, ViewerState, content_rows, make_peek_theme,
    render_themed_status_line, with_alternate_screen,
};

// ---------------------------------------------------------------------------
// Help keys
// ---------------------------------------------------------------------------

const HEX_HELP_KEYS_STANDALONE: &[(&str, &str)] = &[
    ("q / Esc", "Quit"),
    ("Up / k", "Scroll up"),
    ("Down / j", "Scroll down"),
    ("PgUp / PgDn", "Page scroll"),
    ("Space", "Page down"),
    ("Home / g", "Top"),
    ("End / G", "Bottom"),
    ("Tab", "Toggle hex / file info"),
    ("i", "File info"),
    ("h / ?", "Toggle help"),
    ("t", "Next theme"),
];

const HEX_HELP_KEYS_TOGGLE: &[(&str, &str)] = &[
    ("q / Esc", "Quit"),
    ("x", "Exit hex mode"),
    ("Up / k", "Scroll up"),
    ("Down / j", "Scroll down"),
    ("PgUp / PgDn", "Page scroll"),
    ("Space", "Page down"),
    ("Home / g", "Top"),
    ("End / G", "Bottom"),
    ("Tab", "Toggle hex / file info"),
    ("i", "File info"),
    ("h / ?", "Toggle help"),
    ("t", "Next theme"),
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
        file_type: &FileType,
        start_offset: u64,
        return_on_x: bool,
    ) -> Result<()> {
        with_alternate_screen(|stdout| {
            run_hex_loop(
                stdout,
                source,
                file_type,
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
pub(crate) fn run_hex_loop(
    stdout: &mut io::Stdout,
    source: &InputSource,
    file_type: &FileType,
    initial_theme: PeekThemeName,
    start_offset: u64,
    return_on_x: bool,
) -> Result<()> {
    let bs = source.open_byte_source()?;
    let total_len = bs.len();

    let help_keys: &'static [(&str, &str)] = if return_on_x {
        HEX_HELP_KEYS_TOGGLE
    } else {
        HEX_HELP_KEYS_STANDALONE
    };

    // ViewerState reused for Info/Help/theme plumbing. Content lines are
    // empty: we draw the hex content ourselves when view_mode == Content.
    let mut state = ViewerState::new(source, file_type, initial_theme, Vec::new(), help_keys)?;

    let name = source.name().to_string();

    let mut top_offset = {
        let (cols, _) = terminal::size().unwrap_or((80, 24));
        align_down(start_offset, bytes_per_row(cols))
    };

    let redraw =
        |stdout: &mut io::Stdout, state: &ViewerState, top_offset: u64| -> Result<()> {
            let (cols, _) = terminal::size().unwrap_or((80, 24));
            let bpr = bytes_per_row(cols);
            let status = render_status_line(&name, state, top_offset, total_len, return_on_x);
            if state.view_mode == ViewMode::Content {
                draw_hex(stdout, &*bs, top_offset, bpr, &state.peek_theme, &status)?;
            } else {
                state.draw(stdout, &status)?;
            }
            Ok(())
        };

    redraw(stdout, &state, top_offset)?;

    loop {
        match event::read()? {
            Event::Key(key) => {
                // Content-mode scrolling is byte-based; intercept before
                // ViewerState's line-based handling.
                if state.view_mode == ViewMode::Content {
                    let (cols, _) = terminal::size().unwrap_or((80, 24));
                    let bpr = bytes_per_row(cols);
                    if let Some(new_top) = handle_content_scroll(key, top_offset, bpr, total_len) {
                        top_offset = new_top;
                        redraw(stdout, &state, top_offset)?;
                        continue;
                    }
                }
                match state.handle_key(key) {
                    KeyAction::Quit => return Ok(()),
                    KeyAction::Redraw | KeyAction::ThemeChanged => {
                        redraw(stdout, &state, top_offset)?;
                    }
                    KeyAction::SwitchToHex => {
                        if return_on_x {
                            return Ok(());
                        }
                        // Standalone hex: x is a no-op
                    }
                    KeyAction::Unhandled(_) => {}
                }
            }
            Event::Resize(_, _) => {
                let (cols, _) = terminal::size().unwrap_or((80, 24));
                top_offset = align_down(top_offset, bytes_per_row(cols));
                redraw(stdout, &state, top_offset)?;
            }
            _ => {}
        }
    }
}

/// Translate Content-mode keys into a new top byte offset. Returns `None`
/// if the key isn't a content-scroll key (caller should fall through to
/// `ViewerState::handle_key`).
fn handle_content_scroll(key: KeyEvent, top: u64, bpr: usize, total_len: u64) -> Option<u64> {
    let bpr_u = bpr as u64;
    let rows = content_rows() as u64;
    let max = max_top(total_len, bpr, content_rows());
    let new_top = match key.code {
        KeyCode::Up | KeyCode::Char('k') => top.saturating_sub(bpr_u),
        KeyCode::Down | KeyCode::Char('j') => top.saturating_add(bpr_u).min(max),
        KeyCode::PageUp => top.saturating_sub(bpr_u.saturating_mul(rows.saturating_sub(1))),
        KeyCode::PageDown | KeyCode::Char(' ') => top
            .saturating_add(bpr_u.saturating_mul(rows.saturating_sub(1)))
            .min(max),
        KeyCode::Home | KeyCode::Char('g') => 0,
        KeyCode::End | KeyCode::Char('G') => max,
        // Ctrl+C is handled by ViewerState (translates to Quit). Don't intercept.
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return None,
        _ => return None,
    };
    Some(new_top)
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn draw_hex(
    stdout: &mut io::Stdout,
    bs: &dyn ByteSource,
    top_offset: u64,
    bpr: usize,
    theme: &PeekTheme,
    status: &str,
) -> Result<()> {
    let (_cols, total_rows) = terminal::size().unwrap_or((80, 24));
    let rows = (total_rows as usize).saturating_sub(1);

    stdout.write_all(ANSI_RESET_BYTES)?;
    execute!(
        stdout,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0),
    )?;

    let want = rows * bpr;
    let buf = bs.read_range(top_offset, want)?;

    for (i, row) in buf.chunks(bpr).enumerate() {
        if i > 0 {
            stdout.write_all(b"\r\n")?;
        }
        let row_off = top_offset + (i * bpr) as u64;
        let line = format_row(theme, row_off, row, bpr);
        stdout.write_all(line.as_bytes())?;
    }

    stdout.write_all(ANSI_RESET_BYTES)?;
    execute!(stdout, cursor::MoveTo(0, total_rows.saturating_sub(1)))?;
    stdout.write_all(status.as_bytes())?;

    stdout.flush()?;
    Ok(())
}

fn render_status_line(
    name: &str,
    state: &ViewerState,
    top_offset: u64,
    total_len: u64,
    return_on_x: bool,
) -> String {
    let theme = &state.peek_theme;
    let pct = if total_len > 0 {
        (top_offset * 100 / total_len).min(100)
    } else {
        0
    };
    let offset_str = format!("0x{:08x} / 0x{:08x} ({}%)", top_offset, total_len, pct);
    let mode_label = if state.view_mode == ViewMode::Content {
        "hex"
    } else {
        state.view_mode.label()
    };
    let segments: Vec<(&str, _)> = vec![
        (name, theme.accent),
        (mode_label, theme.label),
        (offset_str.as_str(), theme.muted),
        (state.current_theme.cli_name(), theme.muted),
    ];

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
