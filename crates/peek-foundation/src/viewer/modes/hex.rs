use anyhow::Result;
use crossterm::terminal;
use syntect::highlighting::Color;

use super::{Mode, ModeId, Position, RenderCtx, Window};
use crate::input::{ByteSource, InputSource};
use crate::output::PrintOutput;
use crate::theme::PeekTheme;
use crate::viewer::hex::{align_down, bytes_per_row, format_row, max_top};
use crate::viewer::ui::Action;

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
            label: "hex".to_string(),
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
