use std::rc::Rc;

use anyhow::Result;

use super::{Mode, ModeId, RenderCtx};
use crate::input::detect::StructuredFormat;
use crate::theme::ThemeManager;
use crate::viewer::highlight_lines;
use crate::viewer::structured;
use crate::viewer::ui::Action;

/// Content view: text, syntax-highlighted source, pretty-printed structured
/// data, or SVG XML source. Owns the pre-loaded raw text plus a lazy
/// pretty-print slot. When `allow_pretty_toggle` is set, `r` flips
/// between them — used for structured files (JSON/YAML/TOML/XML) where
/// raw vs pretty is a meaningful user choice. SVG XML opts out so `r`
/// instead falls through to cycling between SVG-rasterized and SVG-XML.
///
/// The pretty-print is computed on demand (first time the user lands on
/// pretty). On parse failure we cache the error, fall back to the raw
/// view, and queue a one-shot warning for `ViewerState` to merge into
/// `FileInfo.warnings` so it shows up in the Info view.
pub(crate) struct ContentMode {
    raw: String,
    /// Format to pretty-print as, when one applies. None means "no pretty
    /// form available" (source code, plain text).
    pretty_target: Option<StructuredFormat>,
    /// Lazy pretty-print result. `None` = not yet attempted; `Some(Ok)` =
    /// cached pretty text; `Some(Err)` = parse error captured (the
    /// matching warning was already pushed to `pending_warnings`).
    pretty: Option<Result<String, String>>,
    /// Warnings produced during render that haven't been collected by
    /// `ViewerState` yet — drained on every `take_warnings` call.
    pending_warnings: Vec<String>,
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
        pretty_target: Option<StructuredFormat>,
        syntax_token: Option<String>,
        theme_manager: Rc<ThemeManager>,
        initial_use_pretty: bool,
        allow_pretty_toggle: bool,
        label: &'static str,
    ) -> Self {
        Self {
            raw,
            pretty_target,
            pretty: None,
            pending_warnings: Vec::new(),
            syntax_token,
            theme_manager,
            use_pretty: initial_use_pretty && pretty_target.is_some(),
            allow_pretty_toggle,
            label,
        }
    }

    /// Run pretty-print if it's the first time pretty mode is rendered.
    /// Caches the result; on parse failure pushes one warning and returns
    /// silently (the caller falls back to raw).
    fn ensure_pretty(&mut self) {
        if self.pretty.is_some() {
            return;
        }
        let Some(target) = self.pretty_target else {
            return;
        };
        self.pretty = Some(match structured::pretty_print(&self.raw, target) {
            Ok(s) => Ok(s),
            Err(e) => {
                let format_name = match target {
                    StructuredFormat::Json => "JSON",
                    StructuredFormat::Yaml => "YAML",
                    StructuredFormat::Toml => "TOML",
                    StructuredFormat::Xml => "XML",
                };
                self.pending_warnings.push(format!(
                    "{format_name} parse failed ({e}); showing raw source"
                ));
                Err(e.to_string())
            }
        });
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
        if self.use_pretty {
            self.ensure_pretty();
        }
        let content: &str = if self.use_pretty {
            match self.pretty.as_ref() {
                Some(Ok(s)) => s.as_str(),
                // No pretty available, or parse failed — fall back to raw.
                _ => &self.raw,
            }
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
            && self.pretty_target.is_some()
        {
            self.use_pretty = !self.use_pretty;
            true
        } else {
            false
        }
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }

    /// ContentMode tracks position in line units when showing raw —
    /// the line index then corresponds 1:1 to source lines, so a
    /// switch to Hex (and back) lands on the right byte.
    ///
    /// In pretty mode the line index has no relation to source bytes
    /// (e.g. pretty-printed JSON line 50 may correspond to source byte
    /// 200 or 20000). Tracking would lie, so we opt out: switching
    /// from pretty Content to Hex preserves whatever position Hex
    /// previously had instead of synthesizing a wrong one.
    fn tracks_position(&self) -> bool {
        !self.use_pretty
    }
}
