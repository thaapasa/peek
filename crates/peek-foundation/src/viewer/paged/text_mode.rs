//! Shared `Mode` shell for paged *text* read views (presentation slides,
//! EPUB chapters).
//!
//! These differ from the paged *image* shell [`super::PagedImageMode`]:
//! the entries are text-rich and want per-page search, which PDF / CBZ
//! don't. But the two text readers — `PresentationReader` and
//! `EpubReader` (both in `peek-types`) — were otherwise near-identical:
//! same step / search / pipe-walk / status / resize / cache plumbing,
//! differing only in how a page renders, the page-cache key, the status
//! noun, the extra help rows, and (EPUB) an image-config pre-handle hook
//! plus render-time warnings.
//!
//! [`PagedTextReadMode<R>`] owns that shared plumbing (`current`, the
//! per-page render cache, the active search) and the whole `Mode` impl;
//! [`PagedText`] is the small seam each reader fills. The cache key is an
//! associated type so each reader keeps its exact invalidation inputs
//! (slides re-render on a theme cycle; chapters key on viewport rows +
//! image config) without one shell hardcoding the other's key.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::output::PrintOutput;
use crate::theme::PeekTheme;
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window, step_search};
use crate::viewer::search::{self, SearchState, SearchTarget};
use crate::viewer::ui::{Action, HelpEntry};

use super::{pipe_walk_pages, step_paged};

/// The per-reader seam under [`PagedTextReadMode`]: turn a page index into
/// rendered lines, plus the small bits of identity (cache key, status
/// noun, extra help) that vary between readers.
pub trait PagedText {
    /// Cache key: every input that changes a page's rendered output.
    /// A page re-renders when its stored key no longer equals the current
    /// one. Each reader picks its own inputs (theme name for slides;
    /// viewport rows + image config for chapters).
    type Key: Copy + PartialEq;

    /// Total page count — sizes the render cache at construction.
    fn pages_len(&self) -> usize;

    /// Status-segment noun for the page counter, e.g. `"slide"` / `"ch"`.
    fn page_label(&self) -> &'static str;

    /// Right-side status hint advertising the step keys, e.g.
    /// `"n/p:slide"` / `"n/p:chapter"`. Shown only with >1 page.
    fn nav_hint(&self) -> &'static str;

    /// Mode-local actions for the help screen / dispatch card.
    fn extra_actions(&self) -> &'static [HelpEntry];

    /// Build the current cache key from the live render context.
    fn cache_key(&self, ctx: &RenderCtx) -> Self::Key;

    /// Render page `idx` to lines. Called only on a cache miss.
    fn render_page(&mut self, idx: usize, ctx: &RenderCtx) -> Result<Vec<String>>;

    /// Pre-`handle` hook for reader-specific keys (EPUB's image-config
    /// cycle). `Some(_)` consumes the action before the shared n/p +
    /// search dispatch; default consumes nothing.
    fn pre_handle(&mut self, _action: Action) -> Option<Handled> {
        None
    }

    /// Drain warnings accumulated during rendering. Default: none.
    fn take_warnings(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// One cached page render: the key that produced it and the lines.
struct PageCache<K> {
    key: K,
    lines: Vec<String>,
}

/// Shared paged-text read mode. Generic over the [`PagedText`] reader
/// that supplies per-page rendering and identity.
pub struct PagedTextReadMode<R: PagedText> {
    inner: R,
    current: usize,
    cache: Vec<Option<PageCache<R::Key>>>,
    /// Active text search over the current page's rendered lines. Cleared
    /// on a page step or resize — both change the line set the match
    /// indices point into.
    search: Option<SearchState>,
}

impl<R: PagedText> PagedTextReadMode<R> {
    pub fn new(inner: R) -> Self {
        let n = inner.pages_len();
        let mut cache = Vec::with_capacity(n);
        cache.resize_with(n, || None);
        Self {
            inner,
            current: 0,
            cache,
            search: None,
        }
    }

    fn ensure_rendered(&mut self, ctx: &RenderCtx) -> Result<&[String]> {
        if self.cache.is_empty() {
            return Ok(&[]);
        }
        let idx = self.current;
        let key = self.inner.cache_key(ctx);
        let stale = self.cache[idx].as_ref().is_none_or(|c| c.key != key);
        if stale {
            // `render_page` borrows `inner`, finishing before the cache
            // store — no disjoint-borrow split needed.
            let lines = self.inner.render_page(idx, ctx)?;
            self.cache[idx] = Some(PageCache { key, lines });
        }
        Ok(&self.cache[idx].as_ref().expect("cache populated").lines)
    }

    /// Current page's rendered lines if already cached, else `&[]`. Used
    /// by `total_lines` and `set_search`, which must not trigger a render.
    fn cached_lines(&self) -> &[String] {
        self.cache
            .get(self.current)
            .and_then(|c| c.as_ref())
            .map(|c| c.lines.as_slice())
            .unwrap_or(&[])
    }
}

impl<R: PagedText> Mode for PagedTextReadMode<R> {
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
        let lines = self.ensure_rendered(ctx)?;
        let total = lines.len();
        let mut win = slice_window(lines, scroll, rows);
        search::overlay_window(&mut win, scroll, self.search.as_ref(), ctx.peek_theme);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache
            .get(self.current)
            .and_then(|c| c.as_ref())
            .map(|c| c.lines.len())
    }

    /// Print mode walks every page in order, separating each with a blank
    /// line — the whole document materialises only on the pipe path; the
    /// interactive view stays single-page. Honors the cache so pages
    /// already rendered interactively reuse their output.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let total = self.cache.len();
        let saved = self.current;
        let res = pipe_walk_pages(out, total, |i, out| {
            self.current = i;
            let lines = self.ensure_rendered(ctx)?;
            for line in lines {
                out.write_line(line)?;
            }
            Ok(())
        });
        self.current = saved;
        res
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        self.inner.extra_actions()
    }

    fn handle(&mut self, action: Action) -> Handled {
        // Esc clears an active search before falling through to global back.
        if action == Action::Back && self.search.is_some() {
            self.search = None;
            return Handled::Yes;
        }
        // Reader-specific keys (EPUB image-config cycle) get first refusal.
        if let Some(h) = self.inner.pre_handle(action) {
            return h;
        }
        match action {
            // `n` / `p` step pages — but while a search is active they
            // navigate matches instead (Esc clears the search to get page
            // stepping back).
            Action::Next => {
                if self.search.is_some() {
                    step_search(&mut self.search, 1)
                } else {
                    step_paged(&mut self.current, self.cache.len(), 1)
                }
            }
            Action::Prev => {
                if self.search.is_some() {
                    step_search(&mut self.search, -1)
                } else {
                    step_paged(&mut self.current, self.cache.len(), -1)
                }
            }
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        if self.cache.is_empty() {
            return Vec::new();
        }
        let mut segs = vec![(
            format!(
                "{} {}/{}",
                self.inner.page_label(),
                self.current + 1,
                self.cache.len()
            ),
            theme.muted,
        )];
        if let Some(search) = &self.search {
            segs.push(search.status_segment(theme));
        }
        segs
    }

    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> {
        if self.cache.len() <= 1 {
            return Vec::new();
        }
        vec![self.inner.nav_hint()]
    }

    fn on_resize(&mut self, _term_cols: usize, _term_rows: usize) {
        // A width change re-wraps every page — match line indices no longer
        // line up, so drop the search.
        self.search = None;
    }

    fn set_search(&mut self, query: Option<&str>) -> SearchTarget {
        match query {
            Some(q) if !q.is_empty() => {
                // Scan the current page's rendered lines. The prompt only
                // opens while viewing, so the cache is populated.
                let state = SearchState::scan(self.cached_lines().iter(), q);
                let first = state.first_line();
                self.search = Some(state);
                first.map_or(SearchTarget::Owned, SearchTarget::ScrollTo)
            }
            _ => {
                self.search = None;
                SearchTarget::Owned
            }
        }
    }

    fn take_warnings(&mut self) -> Vec<String> {
        self.inner.take_warnings()
    }
}
