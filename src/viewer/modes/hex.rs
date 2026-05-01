use anyhow::Result;
use crossterm::terminal;
use syntect::highlighting::Color;

use super::{Mode, ModeId, Position, RenderCtx};
use crate::input::{ByteSource, InputSource};
use crate::theme::PeekTheme;
use crate::viewer::hex::{
    align_down, bytes_per_row, format_row, max_top,
};
use crate::viewer::ui::{Action, content_rows};

pub(crate) struct HexMode {
    bs: Box<dyn ByteSource>,
    total_len: u64,
    top_offset: u64,
    label: String,
}

impl HexMode {
    pub(crate) fn new(source: &InputSource, start_offset: u64) -> Result<Self> {
        let bs = source.open_byte_source()?;
        let total_len = bs.len();
        let (cols, _) = terminal::size().unwrap_or((80, 24));
        let top_offset = align_down(start_offset, bytes_per_row(cols));
        Ok(Self {
            bs,
            total_len,
            top_offset,
            label: "hex".to_string(),
        })
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

    fn render(&mut self, ctx: &RenderCtx) -> Result<Vec<String>> {
        let (cols, _) = terminal::size().unwrap_or((80, 24));
        let bpr = bytes_per_row(cols);
        let rows = content_rows();
        let want = rows * bpr;
        let buf = self.bs.read_range(self.top_offset, want)?;

        let mut lines = Vec::with_capacity(rows);
        for (i, row) in buf.chunks(bpr).enumerate() {
            let row_off = self.top_offset + (i * bpr) as u64;
            lines.push(format_row(ctx.peek_theme, row_off, row, bpr));
        }
        Ok(lines)
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        let (cols, _) = terminal::size().unwrap_or((80, 24));
        let bpr = bytes_per_row(cols);
        let bpr_u = bpr as u64;
        let rows = content_rows() as u64;
        let max = max_top(self.total_len, bpr, content_rows());
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

    fn on_resize(&mut self) {
        let (cols, _) = terminal::size().unwrap_or((80, 24));
        self.top_offset = align_down(self.top_offset, bytes_per_row(cols));
    }

    fn tracks_position(&self) -> bool {
        true
    }

    fn position(&self) -> Position {
        Position::Byte(self.top_offset)
    }

    fn set_position(&mut self, pos: Position, source: &InputSource) {
        let byte = match pos {
            Position::Byte(b) => Some(b),
            Position::Line(l) => source.line_to_byte(l),
            Position::Unknown => None,
        };
        if let Some(b) = byte {
            let (cols, _) = terminal::size().unwrap_or((80, 24));
            self.top_offset = align_down(b, bytes_per_row(cols));
        }
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
