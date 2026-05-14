//! RTF read mode. Renders the parsed AST per (width, style_mode) and
//! caches the result. Cache invalidates on resize / color cycle.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::{PeekTheme, StyleMode};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window};
use crate::viewer::search::{self, SearchState};
use crate::viewer::ui::{Action, HelpEntry};

use super::parse::Parsed;
use super::render;

const EXTRA_ACTIONS: &[HelpEntry] = &[
    (&[Action::OpenSearch], "Search"),
    (
        &[Action::NextMatch, Action::PrevMatch],
        "Next / previous match",
    ),
];

#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    width: usize,
    style_mode: StyleMode,
}

struct Cached {
    key: CacheKey,
    lines: Vec<String>,
}

pub(crate) struct RtfReadMode {
    #[allow(dead_code)]
    source: InputSource,
    parsed: Parsed,
    cache: Option<Cached>,
    /// Active text search over the rendered lines. Indices are the
    /// wrapped-line domain, so a resize clears it.
    search: Option<SearchState>,
}

impl RtfReadMode {
    pub(crate) fn new(source: InputSource, parsed: Parsed) -> Self {
        Self {
            source,
            parsed,
            cache: None,
            search: None,
        }
    }

    fn ensure_rendered(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        style_mode: StyleMode,
    ) -> Result<&[String]> {
        let key = CacheKey { width, style_mode };
        let needs = self.cache.as_ref().map(|c| c.key != key).unwrap_or(true);
        if needs {
            let lines = render::render(&self.parsed, width, theme, style_mode)?;
            self.cache = Some(Cached { key, lines });
        }
        Ok(&self.cache.as_ref().expect("cache populated").lines)
    }
}

impl Mode for RtfReadMode {
    fn id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn label(&self) -> &str {
        "Read"
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let lines =
            self.ensure_rendered(ctx.term_cols, ctx.peek_theme, ctx.peek_theme.style_mode)?;
        let total = lines.len();
        let mut win = slice_window(lines, scroll, rows);
        search::overlay_window(&mut win, scroll, self.search.as_ref(), ctx.peek_theme);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache.as_ref().map(|c| c.lines.len())
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let lines =
            self.ensure_rendered(ctx.term_cols, ctx.peek_theme, ctx.peek_theme.style_mode)?;
        for line in lines {
            out.write_line(line)?;
        }
        Ok(())
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
            Action::NextMatch => step_match(&mut self.search, 1),
            Action::PrevMatch => step_match(&mut self.search, -1),
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

/// Step the search cursor and ask the viewer to scroll to the new
/// match's line. `Handled::Yes` (no scroll) when there's no search or
/// no matches.
fn step_match(search: &mut Option<SearchState>, delta: isize) -> Handled {
    match search.as_mut().and_then(|s| s.step(delta)) {
        Some(line) => Handled::YesScrollTo(line),
        None => Handled::Yes,
    }
}
