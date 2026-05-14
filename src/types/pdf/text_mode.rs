//! PDF text-extraction read mode.
//!
//! Mirrors the DOCX / RTF read-mode shape: render the entire document
//! to a width-wrapped `Vec<String>`, cached per (width, style_mode).
//! Page boundaries get a muted `--- Page N ---` separator line so the
//! reader has a visual anchor without leaving the single scroll axis.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::output::PrintOutput;
use crate::theme::{PeekTheme, StyleMode};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window, step_search};
use crate::viewer::search::{self, SearchState};
use crate::viewer::ui::{Action, HelpEntry};

use super::package::Doc;

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

pub(crate) struct PdfTextMode {
    doc: Doc,
    cache: Option<Cached>,
    warnings: Vec<String>,
    /// Active text search over the rendered text. Match line indices
    /// are the wrapped-line domain, so a resize clears it.
    search: Option<SearchState>,
}

impl PdfTextMode {
    pub(crate) fn new(doc: Doc) -> Self {
        Self {
            doc,
            cache: None,
            warnings: Vec::new(),
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
            let lines = render_text(&self.doc, width, theme, &mut self.warnings);
            self.cache = Some(Cached { key, lines });
        }
        Ok(&self.cache.as_ref().expect("cache populated").lines)
    }
}

/// Wrap each page's text at `width`, separated by a muted page marker.
/// Errors per page degrade to a single placeholder line so a corrupt
/// page doesn't kill the whole render.
fn render_text(
    doc: &Doc,
    width: usize,
    theme: &PeekTheme,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    let total = doc.page_count();
    for idx in 0..total {
        if idx > 0 {
            out.push(String::new());
            out.push(theme.paint_muted(&format!("--- Page {} ---", idx + 1)));
            out.push(String::new());
        }
        match doc.page_text(idx) {
            Ok(text) => {
                for line in text.lines() {
                    push_wrapped(&mut out, line, width.max(20));
                }
            }
            Err(e) => {
                warnings.push(format!("page {}: text extract failed: {e:#}", idx + 1));
                out.push(theme.paint_warning(&format!("[page {} text unavailable]", idx + 1)));
            }
        }
    }
    out
}

/// Greedy word-wrap by character count. Falls back to mid-word breaks
/// for tokens longer than `width` so a giant URL or compound word
/// doesn't overflow the terminal line.
fn push_wrapped(out: &mut Vec<String>, line: &str, width: usize) {
    if line.is_empty() {
        out.push(String::new());
        return;
    }
    let mut buf = String::new();
    let mut col = 0usize;
    for word in line.split_whitespace() {
        let word_len = word.chars().count();
        if word_len > width {
            if !buf.is_empty() {
                out.push(std::mem::take(&mut buf));
                col = 0;
            }
            // Hard-split the long word at width boundaries.
            let mut chars = word.chars();
            loop {
                let chunk: String = chars.by_ref().take(width).collect();
                if chunk.is_empty() {
                    break;
                }
                out.push(chunk);
            }
            continue;
        }
        let needed = if col == 0 { word_len } else { word_len + 1 };
        if col + needed > width {
            out.push(std::mem::take(&mut buf));
            col = 0;
        }
        if col > 0 {
            buf.push(' ');
            col += 1;
        }
        buf.push_str(word);
        col += word_len;
    }
    if !buf.is_empty() {
        out.push(buf);
    }
}

impl Mode for PdfTextMode {
    fn id(&self) -> ModeId {
        ModeId::Content
    }

    fn label(&self) -> &str {
        "Text"
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
        // A width change re-wraps the text, so match line indices no
        // longer line up — drop the search.
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

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}
