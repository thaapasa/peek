use anyhow::Result;

use super::{Mode, ModeId, RenderCtx};

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

    fn render(&mut self, ctx: &RenderCtx) -> Result<Vec<String>> {
        Ok(crate::info::render(ctx.file_info, ctx.peek_theme, ctx.render_opts))
    }
}
