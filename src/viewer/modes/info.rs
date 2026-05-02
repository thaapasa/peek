use anyhow::Result;

use super::{Mode, ModeId, RenderCtx, Window};

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

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, _rows: usize) -> Result<Window> {
        let lines = crate::info::render(ctx.file_info, ctx.peek_theme, ctx.render_opts);
        let total = lines.len();
        Ok(Window { lines, total })
    }
}
