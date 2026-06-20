//! Hex dump: `HexMode` (interactive viewport-clamped rendering and
//! pipe-mode full-file streaming) plus the layout primitives and the
//! per-row formatter it is built on. Byte-offset scrolling — the mode
//! owns its scroll and tracks position in byte units so a switch to a
//! line-based mode (and back) lands on the right spot.

use std::fmt::Write as _;

use anyhow::Result;
use crossterm::terminal;
use syntect::highlighting::Color;

use super::{Mode, ModeId, Position, RenderCtx, Window};
use crate::output::PrintOutput;
use crate::viewer::ui::Action;
use peek_io::{ByteSource, InputSource};
use peek_theme::PeekTheme;

pub struct HexMode {
    bs: Box<dyn ByteSource>,
    total_len: u64,
    top_offset: u64,
    label: String,
    /// Last terminal width / content-row count seen — set on every
    /// `render`/`on_resize`. `scroll` and `set_position` are called
    /// outside the render path and read these cached values rather than
    /// querying the terminal directly. The user must have rendered at
    /// least once before they can scroll, so the cache is always seeded.
    cached_cols: u16,
    cached_rows: usize,
    /// Absolute offset of a byte to highlight — the position last jumped to
    /// (e.g. a symbol's location). Set only by [`Mode::jump_position`], so a
    /// plain scroll or a mode-switch position-restore never marks a byte;
    /// persists until the next jump. `None` = no highlight.
    marked: Option<u64>,
}

impl HexMode {
    pub fn new(source: &InputSource, start_offset: u64) -> Result<Self> {
        let bs = source.open_byte_source()?;
        let total_len = bs.len();
        let (cols, rows) = terminal::size().unwrap_or((80, 24));
        let cached_rows = (rows as usize).saturating_sub(1);
        let top_offset = align_down(start_offset, bytes_per_row(cols));
        Ok(Self {
            bs,
            total_len,
            top_offset,
            label: "Hex".to_string(),
            cached_cols: cols,
            cached_rows,
            marked: None,
        })
    }

    /// Resolve a `Position` to an absolute byte offset (Line via the
    /// source's line→byte map). Shared by `set_position` and
    /// `jump_position`.
    fn resolve(pos: Position, source: &InputSource) -> Option<u64> {
        match pos {
            Position::Byte(b) => Some(b),
            Position::Line(l) => source.line_to_byte(l),
            Position::Unknown => None,
        }
    }
}

impl Mode for HexMode {
    fn id(&self) -> ModeId {
        ModeId::Hex
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn is_aux(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, _rows: usize) -> Result<Window> {
        self.cached_cols = ctx.term_cols as u16;
        self.cached_rows = ctx.term_rows;
        let bpr = bytes_per_row(self.cached_cols);
        let rows = self.cached_rows;
        let want = rows.saturating_mul(bpr);
        let buf = self.bs.read_range(self.top_offset, want)?;

        let mut lines = Vec::with_capacity(rows);
        for (i, row) in buf.chunks(bpr).enumerate() {
            let row_off = self.top_offset + (i * bpr) as u64;
            // Row-relative index of the marked byte, if it falls in this row.
            let mark = self.marked.and_then(|m| {
                (m >= row_off && m < row_off + bpr as u64).then(|| (m - row_off) as usize)
            });
            lines.push(format_row(ctx.peek_theme, row_off, row, bpr, mark));
        }
        let total = lines.len();
        Ok(Window { lines, total })
    }

    /// Stream the full file to the print sink. Reading in 4 KB-sized
    /// chunks (256 rows at the typical 16-bpr layout) avoids ever
    /// holding more than one chunk in memory — important for hex-dumping
    /// multi-GB binaries.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let bpr = bytes_per_row(ctx.term_cols as u16);
        let chunk_bytes = bpr * 256;
        let mut offset: u64 = 0;
        while offset < self.total_len {
            let buf = self.bs.read_range(offset, chunk_bytes)?;
            if buf.is_empty() {
                break;
            }
            for (i, row) in buf.chunks(bpr).enumerate() {
                let row_off = offset + (i * bpr) as u64;
                out.write_line(&format_row(ctx.peek_theme, row_off, row, bpr, None))?;
            }
            offset += buf.len() as u64;
        }
        Ok(())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        let bpr = bytes_per_row(self.cached_cols);
        let bpr_u = bpr as u64;
        let rows = self.cached_rows as u64;
        let max = max_top(self.total_len, bpr, self.cached_rows);
        let new_top = match action {
            Action::ScrollUp => self.top_offset.saturating_sub(bpr_u),
            Action::ScrollDown => self.top_offset.saturating_add(bpr_u).min(max),
            Action::PageUp => self
                .top_offset
                .saturating_sub(bpr_u.saturating_mul(rows.saturating_sub(1))),
            Action::PageDown => self
                .top_offset
                .saturating_add(bpr_u.saturating_mul(rows.saturating_sub(1)))
                .min(max),
            Action::Top => 0,
            Action::Bottom => max,
            _ => return false,
        };
        self.top_offset = new_top;
        true
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn on_resize(&mut self, term_cols: usize, term_rows: usize) {
        self.cached_cols = term_cols as u16;
        self.cached_rows = term_rows;
        self.top_offset = align_down(self.top_offset, bytes_per_row(self.cached_cols));
    }

    fn tracks_position(&self) -> bool {
        true
    }

    fn position(&self) -> Position {
        Position::Byte(self.top_offset)
    }

    fn set_position(&mut self, pos: Position, source: &InputSource) {
        if let Some(b) = Self::resolve(pos, source) {
            self.top_offset = align_down(b, bytes_per_row(self.cached_cols));
        }
    }

    fn jump_position(&mut self, pos: Position, source: &InputSource) {
        // Scroll there and mark the exact byte (not the row-aligned top) so
        // the jumped-to position is visually pinpointed in the dump.
        self.set_position(pos, source);
        self.marked = Self::resolve(pos, source);
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let pct = (self.top_offset * 100)
            .checked_div(self.total_len)
            .unwrap_or(0)
            .min(100);
        let s = format!(
            "0x{:08x} / 0x{:08x} ({}%)",
            self.top_offset, self.total_len, pct
        );
        vec![(s, theme.muted)]
    }

    fn status_hints(&self, has_return_target: bool) -> Vec<&'static str> {
        if has_return_target {
            vec!["x:exit hex"]
        } else {
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Hex layout helpers
// ---------------------------------------------------------------------------

/// Compute bytes-per-row for a given terminal width. Formula:
///   row width = 14 + 4*bpr
///     (8 offset + 2 spaces + 3*bpr hex (incl. mid-gap) + 2 spaces + 2 pipes + bpr ascii)
/// We pick the largest multiple of 8 (≥ 8) that fits.
fn bytes_per_row(term_cols: u16) -> usize {
    let cols = term_cols as usize;
    let usable = cols.saturating_sub(14);
    let raw = usable / 4;
    ((raw / 8) * 8).max(8)
}

fn align_down(offset: u64, bpr: usize) -> u64 {
    let bpr = bpr as u64;
    if bpr == 0 {
        return 0;
    }
    (offset / bpr) * bpr
}

/// Maximum valid top offset such that the last screen of content is fully
/// utilized. Always aligned to `bpr`.
fn max_top(len: u64, bpr: usize, rows: usize) -> u64 {
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
/// `mark` is the row-relative index of a byte to highlight (jumped-to
/// position) — its hex and ASCII cells get the selection background.
fn format_row(
    theme: &PeekTheme,
    offset: u64,
    bytes: &[u8],
    bpr: usize,
    mark: Option<usize>,
) -> String {
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
            if mark == Some(i) {
                theme.push_bg(&mut out, theme.selection);
            }
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
            let (s, color): (&str, _) = if (0x20..=0x7e).contains(&b) {
                ((b as char).encode_utf8(&mut buf), theme.value)
            } else {
                (".", theme.muted)
            };
            if mark == Some(i) {
                theme.push_bg(&mut out, theme.selection);
                theme.push_fg(&mut out, color);
                out.push_str(s);
                theme.push_reset(&mut out);
            } else {
                theme.paint_into(&mut out, s, color);
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
    use peek_theme::{PeekThemeName, load_embedded_theme, strip_ansi};

    fn test_theme() -> PeekTheme {
        let t = load_embedded_theme(PeekThemeName::IdeaDark.tmtheme_source());
        PeekTheme::from_syntect(&t)
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
        let row1 = strip_ansi(&format_row(&theme, 0, &bytes_0_15, 16, None));
        let row2 = strip_ansi(&format_row(&theme, 16, &bytes_16_31, 16, None));
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
        let row = strip_ansi(&format_row(&theme, 0, &bytes, 16, None));
        // ASCII column should show "Hello, World!!!" then '.' for the newline
        assert!(row.ends_with("|Hello, World!!!.|"));
    }

    #[test]
    fn mark_highlights_without_changing_layout() {
        let theme = test_theme();
        let bytes: Vec<u8> = (0u8..16).collect();
        let plain = format_row(&theme, 0, &bytes, 16, None);
        let marked = format_row(&theme, 0, &bytes, 16, Some(3));
        // Highlight is color-only: the visible text is byte-identical.
        assert_eq!(strip_ansi(&plain), strip_ansi(&marked));
        // ...but the marked row carries extra background escapes.
        assert!(marked.len() > plain.len());
    }

    #[test]
    fn format_row_handles_short_final_row() {
        let theme = test_theme();
        let row = strip_ansi(&format_row(&theme, 0x1000, b"abcde", 16, None));
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
            let row = strip_ansi(&format_row(&theme, 0, &bytes, bpr, None));
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
