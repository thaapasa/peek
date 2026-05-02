use anyhow::Result;

use super::{Mode, ModeId, RenderCtx, Window, slice_window};
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

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let full = render_help_with_keys(ctx.peek_theme, ctx.theme_name, &self.actions);
        let total = full.len();
        let lines = slice_window(&full, scroll, rows);
        Ok(Window { lines, total })
    }
}
