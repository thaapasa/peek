//! Rendered HTML view backed by `html2text`.
//!
//! Renders the whole document into wrapped, ANSI-styled lines on first
//! use and on terminal width changes. Rendering is whole-document
//! (html2text has no streaming API), so very large HTML may pause on
//! first render — typical pages are well under 1 MB and render
//! instantly.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::input::InputSource;
use crate::theme::{PeekTheme, StyleMode};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window, step_search};
use crate::viewer::search::{self, SearchState};
use crate::viewer::ui::{Action, HelpEntry};

use super::render;

const EXTRA_ACTIONS: &[HelpEntry] = &[
    (&[Action::OpenSearch], "Search"),
    (
        &[Action::NextMatch, Action::PrevMatch],
        "Next / previous match",
    ),
];

pub(crate) struct RenderedMode {
    source: InputSource,
    style_mode: StyleMode,
    /// Cached render keyed by the width it was produced for. `None`
    /// before the first render; replaced when `term_cols` changes.
    cache: Option<Cached>,
    /// Active text search over the rendered lines. Indices are the
    /// wrapped-line domain, so a resize clears it.
    search: Option<SearchState>,
}

struct Cached {
    width: usize,
    lines: Vec<String>,
}

impl RenderedMode {
    pub(crate) fn new(source: InputSource, style_mode: StyleMode) -> Self {
        Self {
            source,
            style_mode,
            cache: None,
            search: None,
        }
    }

    fn ensure_rendered(&mut self, width: usize) -> Result<&[String]> {
        let needs_render = self
            .cache
            .as_ref()
            .map(|c| c.width != width)
            .unwrap_or(true);
        if needs_render {
            let bytes = self.source.read_bytes()?;
            let lines = render::render(&bytes, width.max(20), self.style_mode)?;
            self.cache = Some(Cached { width, lines });
        }
        Ok(&self.cache.as_ref().expect("cache populated").lines)
    }
}

impl Mode for RenderedMode {
    fn id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn label(&self) -> &str {
        "Rendered"
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let lines = self.ensure_rendered(ctx.term_cols)?;
        let total = lines.len();
        let mut win = slice_window(lines, scroll, rows);
        search::overlay_window(&mut win, scroll, self.search.as_ref(), ctx.peek_theme);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache.as_ref().map(|c| c.lines.len())
    }

    fn on_resize(&mut self, _term_cols: usize, _term_rows: usize) {
        // A width change re-wraps the document — match line indices no
        // longer line up, so drop the search.
        self.search = None;
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::Back if self.search.is_some() => {
                self.search = None;
                Handled::Yes
            }
            Action::NextMatch => step_search(&mut self.search, 1),
            Action::PrevMatch => step_search(&mut self.search, -1),
            _ => Handled::No,
        }
    }

    fn set_search(&mut self, query: Option<&str>) -> Option<usize> {
        match query {
            Some(q) if !q.is_empty() => {
                let lines = self
                    .cache
                    .as_ref()
                    .map(|c| c.lines.as_slice())
                    .unwrap_or(&[]);
                let state = SearchState::scan(lines.iter(), q);
                let first = state.first_line();
                self.search = Some(state);
                first
            }
            _ => {
                self.search = None;
                None
            }
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        self.search
            .as_ref()
            .map(|s| vec![s.status_segment(theme)])
            .unwrap_or_default()
    }
}
