use anyhow::Result;

use super::{Mode, ModeId, RenderCtx, Window, slice_window};

pub(crate) struct InfoMode;

impl InfoMode {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Mode for InfoMode {
    fn id(&self) -> ModeId {
        ModeId::Info
    }

    fn label(&self) -> &str {
        "Info"
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let rendered = crate::info::render(ctx.file_info, ctx.peek_theme, ctx.render_opts);
        // Info fields are one terminal line each by construction, but a
        // long value (a decode-failure warning, a deep path) can exceed
        // the width. Wrap to the content area so the terminal never
        // soft-wraps a line the ScreenBuffer counts as one row — a desync
        // that leaves the overflow tail painted after the view changes.
        let full: Vec<String> = rendered
            .iter()
            .flat_map(|l| crate::viewer::ui::wrap_styled_words(l, ctx.term_cols))
            .collect();
        let total = full.len();
        let lines = slice_window(&full, scroll, rows);
        Ok(Window { lines, total })
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }
}
