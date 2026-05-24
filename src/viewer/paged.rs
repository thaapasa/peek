//! Shared primitives for paged-render modes (PDF, CBZ, EPUB).
//!
//! Each of those modes presents one entry at a time (page / chapter)
//! and caches its rendered output keyed by viewport size + image
//! config. The cache shape, navigation step logic, and image-config
//! cycle handlers are identical across the three; this module is the
//! single source for those pieces.
//!
//! For paged *image* documents (PDF pages, CBZ pages) the whole `Mode`
//! impl is shared too: [`PagedImageMode<R>`] is generic over a small
//! [`PageRenderer`] trait, mirroring [`crate::viewer::modes::RenderedTextMode`]
//! for text documents. Only the per-page render body — Pdfium raster
//! vs ZIP-entry decode — lives in each format's `page_renderer.rs`.
//! EPUB stays separate by design: chapter search and cover-style inline
//! image rendering would have to be lifted into [`PagedImageMode<R>`]
//! as generic concerns first — neither belongs in PDF / CBZ. Prior
//! `/checkup` rounds decided that's not worth doing for one consumer;
//! [`crate::types::ebook::epub::read_mode::EpubReadMode`] keeps its own
//! `Mode` impl reusing the building blocks here ([`render_cached`],
//! [`step_paged`], [`cycle_image_config`], [`PageCacheKey`]).

use anyhow::Result;
use syntect::highlighting::Color;

use crate::output::PrintOutput;
use crate::theme::{PeekTheme, StyleMode};
use crate::types::image::pipeline::{Background, FitMode, ImageConfig, ImageMode};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window};
use crate::viewer::ui::{Action, HelpEntry};

/// Inputs that affect a single page's rendered output. Stored
/// alongside the cached lines so the cache invalidates automatically
/// when the user cycles color (`c`), background (`b`), image mode
/// (`m`), or fit (`f`) — or when the terminal resizes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageCacheKey {
    pub width: usize,
    pub rows: usize,
    pub style_mode: StyleMode,
    pub image_mode: ImageMode,
    pub background: Background,
    pub fit: FitMode,
}

impl PageCacheKey {
    pub fn build(cfg: &ImageConfig, width: usize, rows: usize, style_mode: StyleMode) -> Self {
        Self {
            width,
            rows,
            style_mode,
            image_mode: cfg.mode,
            background: cfg.background,
            fit: cfg.fit,
        }
    }
}

/// Cached per-entry render: the key that produced it plus the lines.
pub(crate) struct CachedRender {
    pub key: PageCacheKey,
    pub lines: Vec<String>,
}

/// Cap on inline image height in pipe / `--print` mode where
/// `term_rows` is unbounded; otherwise a single page would dominate
/// the output. Shared across paged viewers.
pub(crate) const PIPE_IMAGE_MAX_ROWS: u32 = 30;

/// Translate a `term_rows` value (possibly `usize::MAX` for pipe mode)
/// into a `u32` row count for the image pipeline. Pipe mode is capped
/// at [`PIPE_IMAGE_MAX_ROWS`] so a tall image doesn't dominate output.
pub(crate) fn pipe_rows(rows: usize) -> u32 {
    if rows == usize::MAX {
        PIPE_IMAGE_MAX_ROWS
    } else {
        rows.min(u32::MAX as usize) as u32
    }
}

/// Look up `cache[idx]` and, on miss or key mismatch, render via `f`
/// and store. Returns the cached rendered lines.
///
/// Disjoint-borrows pattern: the caller must split `&mut self.cache`
/// off `self` separately from any fields the closure captures, so the
/// closure doesn't collide with the cache borrow held here. Per-mode
/// render bodies therefore become free helpers taking the fields they
/// need by explicit ref rather than `&mut self`.
pub(crate) fn render_cached<F>(
    cache: &mut [Option<CachedRender>],
    idx: usize,
    key: PageCacheKey,
    f: F,
) -> Result<&[String]>
where
    F: FnOnce(&PageCacheKey) -> Result<Vec<String>>,
{
    let stale = cache
        .get(idx)
        .and_then(|c| c.as_ref())
        .is_none_or(|c| c.key != key);
    if stale {
        let lines = f(&key)?;
        cache[idx] = Some(CachedRender { key, lines });
    }
    Ok(&cache[idx].as_ref().expect("cache populated").lines)
}

/// Move `current` by `delta` clamped to `[0, count)`.
///
/// Returns `Handled::No` for an empty list, `Handled::Yes` for a no-op
/// step (already at the bound), `Handled::YesResetScroll` after a real
/// move.
pub(crate) fn step_paged(current: &mut usize, count: usize, delta: i32) -> Handled {
    if count == 0 {
        return Handled::No;
    }
    let max = count - 1;
    let next = if delta >= 0 {
        (*current).saturating_add(delta as usize).min(max)
    } else {
        (*current).saturating_sub(delta.unsigned_abs() as usize)
    };
    if next == *current {
        return Handled::Yes;
    }
    *current = next;
    Handled::YesResetScroll
}

/// Handle the five image-config cycle keys. Returns `Some(Handled::Yes)`
/// when the action matches one of them; `None` when it doesn't (caller
/// continues its own `match`).
pub(crate) fn cycle_image_config(action: Action, cfg: &mut ImageConfig) -> Option<Handled> {
    match action {
        Action::CycleBackground => {
            cfg.background = cfg.background.next();
            Some(Handled::Yes)
        }
        Action::CycleBackgroundBack => {
            cfg.background = cfg.background.prev();
            Some(Handled::Yes)
        }
        Action::CycleImageMode => {
            cfg.mode = cfg.mode.next();
            Some(Handled::Yes)
        }
        Action::CycleImageModeBack => {
            cfg.mode = cfg.mode.prev();
            Some(Handled::Yes)
        }
        Action::CycleFitMode => {
            cfg.fit = cfg.fit.next();
            Some(Handled::Yes)
        }
        _ => None,
    }
}

/// Help rows for the five image-config cycle keys that
/// [`cycle_image_config`] handles. Kept next to the handler — and
/// pinned to it by `image_config_help_pinned_to_handler` — so an image
/// mode's help screen and its key handling cannot drift. Image /
/// animation / paged modes splice these into their own `extra_actions`;
/// EPUB overrides the background / render-mode labels (its keys only
/// bite on cover-image chapters).
pub(crate) const CYCLE_BACKGROUND_HELP: HelpEntry = (
    &[Action::CycleBackground, Action::CycleBackgroundBack],
    "Cycle background",
);
pub(crate) const CYCLE_IMAGE_MODE_HELP: HelpEntry = (
    &[Action::CycleImageMode, Action::CycleImageModeBack],
    "Cycle render mode",
);
pub(crate) const CYCLE_FIT_HELP: HelpEntry = (
    &[Action::CycleFitMode],
    "Cycle fit (contain / width / height)",
);

/// Mode-local help entries for [`PagedImageMode`]: page navigation plus
/// the shared image-config block.
const EXTRA_ACTIONS: &[HelpEntry] = &[
    (
        &[Action::NextChapter, Action::PrevChapter],
        "Next / previous page",
    ),
    CYCLE_BACKGROUND_HELP,
    CYCLE_IMAGE_MODE_HELP,
    CYCLE_FIT_HELP,
];

/// Renders one page of a paged-image document to ASCII-art lines.
///
/// Implementors own the page source — a Pdfium handle, a CBZ ZIP path
/// list — and turn page `idx` into rendered lines. `render_page` takes
/// `&self`: the page source is immutable, and per-render warnings flow
/// out through the `warnings` sink instead of mutating the renderer.
/// [`PagedImageMode<R>`] supplies everything else: the page cache,
/// navigation, image-config cycling, and the whole `Mode` impl.
pub(crate) trait PageRenderer {
    /// Total page count.
    fn page_count(&self) -> usize;

    /// Render page `idx` at the viewport / image-config encoded in
    /// `key`, given the live `config`. Render failures should degrade
    /// to a placeholder line plus a pushed warning, not an `Err`.
    fn render_page(
        &self,
        idx: usize,
        config: ImageConfig,
        key: &PageCacheKey,
        warnings: &mut Vec<String>,
    ) -> Result<Vec<String>>;
}

/// Paged-image read mode generic over its [`PageRenderer`].
///
/// Shows one page at a time through the image pipeline; `n` / `p` step
/// pages, `b` / `m` / `f` cycle image config. Per-page render cache is
/// keyed by viewport size + image config. Mirrors
/// [`crate::viewer::modes::RenderedTextMode`] for text documents.
pub(crate) struct PagedImageMode<R: PageRenderer> {
    renderer: R,
    image_config: ImageConfig,
    current: usize,
    cache: Vec<Option<CachedRender>>,
    warnings: Vec<String>,
}

impl<R: PageRenderer> PagedImageMode<R> {
    pub(crate) fn new(renderer: R, image_config: ImageConfig) -> Self {
        let count = renderer.page_count();
        let mut cache = Vec::with_capacity(count);
        cache.resize_with(count, || None);
        Self {
            renderer,
            image_config,
            current: 0,
            cache,
            warnings: Vec::new(),
        }
    }

    fn ensure_rendered(
        &mut self,
        width: usize,
        rows: usize,
        style_mode: StyleMode,
    ) -> Result<&[String]> {
        if self.renderer.page_count() == 0 {
            return Ok(&[]);
        }
        let idx = self.current;
        let key = PageCacheKey::build(&self.image_config, width, rows, style_mode);
        // Disjoint-borrow split: the render closure captures
        // `&self.renderer` and `&mut self.warnings` while `render_cached`
        // holds `&mut self.cache` — all distinct fields.
        let renderer = &self.renderer;
        let config = self.image_config;
        let warnings = &mut self.warnings;
        render_cached(&mut self.cache, idx, key, |k| {
            renderer.render_page(idx, config, k, warnings)
        })
    }
}

impl<R: PageRenderer> Mode for PagedImageMode<R> {
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
            self.ensure_rendered(ctx.term_cols, ctx.term_rows, ctx.peek_theme.style_mode)?;
        let total = lines.len();
        let win = slice_window(lines, scroll, rows);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache
            .get(self.current)
            .and_then(|c| c.as_ref())
            .map(|c| c.lines.len())
    }

    /// Print mode walks every page in order, separated by a blank line.
    /// Honors the cache so already-rendered pages reuse their output;
    /// the interactive view stays single-page.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let total = self.renderer.page_count();
        let saved = self.current;
        for i in 0..total {
            self.current = i;
            let lines =
                self.ensure_rendered(ctx.term_cols, ctx.term_rows, ctx.peek_theme.style_mode)?;
            for line in lines {
                out.write_line(line)?;
            }
            if i + 1 < total {
                out.write_line("")?;
            }
        }
        self.current = saved;
        Ok(())
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        if let Some(h) = cycle_image_config(action, &mut self.image_config) {
            return h;
        }
        let count = self.renderer.page_count();
        match action {
            Action::NextChapter => step_paged(&mut self.current, count, 1),
            Action::PrevChapter => step_paged(&mut self.current, count, -1),
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let count = self.renderer.page_count();
        if count == 0 {
            return Vec::new();
        }
        vec![(format!("page {}/{}", self.current + 1, count), theme.muted)]
    }

    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> {
        if self.renderer.page_count() <= 1 {
            return Vec::new();
        }
        vec!["n/p:page"]
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_paged_advances_and_clamps() {
        let mut cur = 0;
        assert_eq!(step_paged(&mut cur, 3, 1), Handled::YesResetScroll);
        assert_eq!(cur, 1);
        assert_eq!(step_paged(&mut cur, 3, 1), Handled::YesResetScroll);
        assert_eq!(cur, 2);
        // Already at end: no-op.
        assert_eq!(step_paged(&mut cur, 3, 1), Handled::Yes);
        assert_eq!(cur, 2);
        assert_eq!(step_paged(&mut cur, 3, -1), Handled::YesResetScroll);
        assert_eq!(cur, 1);
        // Backward past zero: clamps to 0.
        assert_eq!(step_paged(&mut cur, 3, -5), Handled::YesResetScroll);
        assert_eq!(cur, 0);
        assert_eq!(step_paged(&mut cur, 3, -1), Handled::Yes);
        assert_eq!(cur, 0);
    }

    #[test]
    fn step_paged_empty_list() {
        let mut cur = 0;
        assert_eq!(step_paged(&mut cur, 0, 1), Handled::No);
    }

    #[test]
    fn pipe_rows_caps_unbounded() {
        assert_eq!(pipe_rows(usize::MAX), PIPE_IMAGE_MAX_ROWS);
        assert_eq!(pipe_rows(42), 42);
    }

    /// The shared image-config help rows and `cycle_image_config` (the
    /// handler every image mode dispatches through) must agree on the
    /// key set — otherwise a mode's help screen advertises a key it
    /// ignores, or vice versa.
    #[test]
    fn image_config_help_pinned_to_handler() {
        let mut cfg = ImageConfig {
            mode: ImageMode::from_str("block"),
            width: 0,
            background: Background::from_str("auto"),
            margin: 0,
            style_mode: StyleMode::Plain,
            edge_density: 0.1,
            fit: FitMode::Contain,
        };
        // Every key the shared help rows advertise is one the handler
        // actually consumes.
        for (keys, _) in [CYCLE_BACKGROUND_HELP, CYCLE_IMAGE_MODE_HELP, CYCLE_FIT_HELP] {
            for &action in keys {
                assert!(
                    cycle_image_config(action, &mut cfg).is_some(),
                    "{action:?} is advertised in a CYCLE_*_HELP row but cycle_image_config ignores it",
                );
            }
        }
        // Unrelated keys fall through untouched.
        for action in [Action::NextMatch, Action::OpenSearch, Action::Back] {
            assert!(cycle_image_config(action, &mut cfg).is_none());
        }
    }
}
