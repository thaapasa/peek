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

    fn is_aux(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let full = crate::info::render(ctx.file_info, ctx.peek_theme, ctx.render_opts);
        let total = full.len();
        let lines = slice_window(&full, scroll, rows);
        Ok(Window { lines, total })
    }
}
