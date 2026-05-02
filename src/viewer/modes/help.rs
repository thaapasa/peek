use anyhow::Result;

use super::{Mode, ModeId, RenderCtx, Window};
use crate::viewer::ui::Action;
use crate::viewer::ui::help::render_help_with_keys;

pub(crate) struct HelpMode {
    actions: Vec<(Action, &'static str)>,
}

impl HelpMode {
    pub(crate) fn new(actions: Vec<(Action, &'static str)>) -> Self {
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

    fn is_aux(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, _rows: usize) -> Result<Window> {
        let lines = render_help_with_keys(ctx.peek_theme, ctx.theme_name, &self.actions);
        let total = lines.len();
        Ok(Window { lines, total })
    }
}
