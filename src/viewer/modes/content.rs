use std::rc::Rc;

use anyhow::Result;

use super::{Mode, ModeId, RenderCtx};
use crate::theme::ThemeManager;
use crate::viewer::highlight_lines;
use crate::viewer::ui::Action;

/// Content view: text, syntax-highlighted source, pretty-printed structured
/// data, or SVG XML source. Owns pre-loaded raw text plus an optional
/// pretty-printed variant. When `allow_pretty_toggle` is set, `r` flips
/// between them — used for structured files (JSON/YAML/TOML/XML) where
/// raw vs pretty is a meaningful user choice. SVG XML opts out so `r`
/// instead falls through to cycling between SVG-rasterized and SVG-XML.
pub(crate) struct ContentMode {
    raw: String,
    pretty: Option<String>,
    syntax_token: Option<String>,
    theme_manager: Rc<ThemeManager>,
    use_pretty: bool,
    allow_pretty_toggle: bool,
    label: &'static str,
}

const RAW_TOGGLE_ACTIONS: &[(Action, &str)] =
    &[(Action::ToggleRawSource, "Toggle raw / pretty")];

impl ContentMode {
    pub(crate) fn new(
        raw: String,
        pretty: Option<String>,
        syntax_token: Option<String>,
        theme_manager: Rc<ThemeManager>,
        initial_use_pretty: bool,
        allow_pretty_toggle: bool,
        label: &'static str,
    ) -> Self {
        Self {
            raw,
            pretty,
            syntax_token,
            theme_manager,
            use_pretty: initial_use_pretty,
            allow_pretty_toggle,
            label,
        }
    }
}

impl Mode for ContentMode {
    fn id(&self) -> ModeId {
        ModeId::Content
    }

    fn label(&self) -> &str {
        self.label
    }

    fn render(&mut self, ctx: &RenderCtx) -> Result<Vec<String>> {
        let content: &str = if self.use_pretty {
            self.pretty.as_deref().unwrap_or(&self.raw)
        } else {
            &self.raw
        };
        if let Some(ref token) = self.syntax_token {
            highlight_lines(content, token, &self.theme_manager, ctx.theme_name)
        } else {
            Ok(content.lines().map(String::from).collect())
        }
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        if self.allow_pretty_toggle {
            RAW_TOGGLE_ACTIONS
        } else {
            &[]
        }
    }

    fn handle(&mut self, action: Action) -> bool {
        if action == Action::ToggleRawSource
            && self.allow_pretty_toggle
            && self.pretty.is_some()
        {
            self.use_pretty = !self.use_pretty;
            true
        } else {
            false
        }
    }

    /// ContentMode tracks position in line units; it doesn't own its
    /// scroll, so `position()` and `set_position()` use the trait
    /// defaults — `ViewerState` reads/writes the line via its own
    /// `scroll[active]` slot.
    fn tracks_position(&self) -> bool {
        true
    }
}
