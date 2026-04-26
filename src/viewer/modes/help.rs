use anyhow::Result;

use super::{Mode, ModeId, RenderCtx};
use crate::viewer::ui::Action;
use crate::viewer::ui::help::render_help_with_keys;

pub(crate) struct HelpMode {
    actions: &'static [(Action, &'static str)],
}

impl HelpMode {
    pub(crate) fn new(actions: &'static [(Action, &'static str)]) -> Self {
        Self { actions }
    }
}

impl Mode for HelpMode {
    fn id(&self) -> ModeId {
        ModeId::Help
    }

    fn label(&self) -> &str {
        "Help"
    }

    fn render(&mut self, ctx: &RenderCtx) -> Result<Vec<String>> {
        Ok(render_help_with_keys(
            ctx.peek_theme,
            ctx.theme_name,
            self.actions,
        ))
    }
}
