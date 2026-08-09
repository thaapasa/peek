//! Shared primitives for paged-render modes (PDF, CBZ, EPUB).
//!
//! Each of those modes presents one entry at a time (page / chapter)
//! and caches its rendered output keyed by viewport size + image
//! config. The cache shape, navigation step logic, and image-config
//! cycle handlers are identical across the three; this module is the
//! single source for those pieces.
//!
//! For paged *image* documents (PDF pages, CBZ pages) the whole `Mode`
//! impl is shared too: [`PagedImageMode<R>`] (in [`mode`]) is generic
//! over a small [`PageRenderer`] trait, mirroring
//! [`crate::viewer::modes::RenderedTextMode`] for text documents. The
//! renderer owns whatever per-page caching its source needs (decoded
//! source bitmap for CBZ, rasterized effective grid for PDF) and
//! returns the viewport-sized ASCII for the current zoom/scroll state;
//! [`PagedImageMode<R>`] handles navigation, zoom, pan, and the `Mode`
//! impl itself but no longer caches rendered output of its own.
//!
//! Paged *text* documents (presentation slides, EPUB chapters) get their
//! own shared `Mode` impl: [`PagedTextReadMode<R>`] (in [`text_mode`])
//! over the [`PagedText`] seam. They want per-page search and a text
//! render cache — concerns that don't belong on the image shell (PDFs
//! don't search per page; comics never render text) — so the two shells
//! stay distinct, each over the navigation building blocks here
//! ([`step_paged`], [`pipe_walk_pages`], and for the image readers
//! [`render_cached`] / [`cycle_image_config`] / [`PageCacheKey`]).

use anyhow::Result;
use peek_theme::StyleMode;

use crate::output::PrintOutput;
use crate::viewer::cell_size;
use crate::viewer::image_render::{
    Background, FitMode, ImageConfig, ImageMode, TermSize, ZoomLevel,
};
use crate::viewer::modes::Handled;
use crate::viewer::ui::{Action, HelpEntry};

mod mode;
mod text_mode;

pub use mode::PagedImageMode;
pub use text_mode::{PagedText, PagedTextReadMode};

/// Inputs that affect a single page's rendered output. Stored
/// alongside the cached lines so the cache invalidates automatically
/// when the user cycles color (`c`), background (`b`), image mode
/// (`m`), or fit (`f`) — or when the terminal resizes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PageCacheKey {
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
pub struct CachedRender {
    pub key: PageCacheKey,
    pub lines: Vec<String>,
}

/// Cap on inline image height in pipe / `--print` mode where
/// `term_rows` is unbounded; otherwise a single page would dominate
/// the output. Shared across paged viewers.
pub const PIPE_IMAGE_MAX_ROWS: u32 = 30;

/// Pipe-mode walk shared by paged viewers (`PagedImageMode`,
/// `EpubReadMode`): emit each page in order, separated by a blank
/// line. The caller's `emit_page` closure picks how to render index
/// `i` and write to `out` — typically setting some `current` cursor,
/// reading from a render cache, and writing the lines.
pub fn pipe_walk_pages<F>(out: &mut PrintOutput, total: usize, mut emit_page: F) -> Result<()>
where
    F: FnMut(usize, &mut PrintOutput) -> Result<()>,
{
    for i in 0..total {
        emit_page(i, out)?;
        if i + 1 < total {
            out.write_line("")?;
        }
    }
    Ok(())
}

/// Translate a `term_rows` value (possibly `usize::MAX` for pipe mode)
/// into a `u32` row count for the image pipeline. Pipe mode is capped
/// at [`PIPE_IMAGE_MAX_ROWS`] so a tall image doesn't dominate output.
pub fn pipe_rows(rows: usize) -> u32 {
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
pub fn render_cached<F>(
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
pub fn step_paged(current: &mut usize, count: usize, delta: i32) -> Handled {
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
pub fn cycle_image_config(action: Action, cfg: &mut ImageConfig) -> Option<Handled> {
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
pub const CYCLE_BACKGROUND_HELP: HelpEntry = (
    &[Action::CycleBackground, Action::CycleBackgroundBack],
    "Cycle background",
);
pub const CYCLE_IMAGE_MODE_HELP: HelpEntry = (
    &[Action::CycleImageMode, Action::CycleImageModeBack],
    "Cycle render mode",
);
pub const CYCLE_FIT_HELP: HelpEntry = (
    &[Action::CycleFitMode],
    "Cycle fit (contain / width / height)",
);

/// Viewport-sized ASCII render of one page at the current zoom + pan
/// state, plus the effective-grid dimensions (= base × zoom) so the
/// caller can derive scroll bounds without measuring `lines` itself.
pub struct PagedRender {
    /// Rendered cells, sized to the visible viewport.
    pub lines: Vec<String>,
    /// Effective grid (base × zoom) in cells. Used for scroll bounds
    /// and the status-line denominator.
    pub effective_cols: u32,
    pub effective_rows: u32,
    /// Visible viewport in cells — what `lines` actually covers. May
    /// be smaller than `term` when the effective grid is smaller, or
    /// equal to `term` when the page overflows.
    pub viewport_cols: u32,
    pub viewport_rows: u32,
}

/// Viewport + zoom + pan state for one paged render call. Bundled
/// because every `render_page` call carries the same shape and the
/// trait method would otherwise drag a wide argument list through
/// every implementor.
#[derive(Copy, Clone, Debug)]
pub struct RenderArgs {
    /// Unscaled viewport (`zoom = 1` base). Implementors multiply by
    /// `zoom.factor()` when sizing their source rasterisation.
    pub term: TermSize,
    pub zoom: ZoomLevel,
    pub scroll_x: u32,
    pub scroll_y: u32,
    pub style_mode: StyleMode,
    /// Reconstructed-text overlay (`o`): renderers with a text layer
    /// (PDF) overwrite rendered glyph cells with the document's real
    /// words at their page positions. Renderers without a text layer
    /// ignore the flag.
    pub text_overlay: bool,
}

/// Renders the visible viewport of one page of a paged-image document
/// to ASCII-art lines.
///
/// Implementors own the page source — a Pdfium handle, a CBZ ZIP path
/// list — and turn page `idx` into a [`PagedRender`] covering the
/// current viewport at the requested zoom / pan. Per-render warnings
/// flow out through the `warnings` sink instead of mutating the
/// renderer. Each implementor owns whatever caching its source needs
/// (decoded native-resolution bitmap for CBZ, rasterized effective
/// grid for PDF) — [`PagedImageMode<R>`] no longer caches rendered
/// output of its own, so the renderer must keep redraws cheap during
/// pan and zoom.
pub trait PageRenderer {
    /// Total page count.
    fn page_count(&self) -> usize;

    /// Whether this renderer can honor [`RenderArgs::text_overlay`] —
    /// true only when the source carries a text layer with positions
    /// (PDF). Gates the `o` toggle and its help row so image-only
    /// sources (CBZ, scans) don't advertise a dead key.
    fn supports_text_overlay(&self) -> bool {
        false
    }

    /// Render page `idx` for the visible viewport at the requested zoom
    /// / pan state in `args`. Render failures should degrade to a
    /// placeholder line plus a pushed warning, not an `Err`.
    fn render_page(
        &self,
        idx: usize,
        config: ImageConfig,
        args: RenderArgs,
        warnings: &mut Vec<String>,
    ) -> Result<PagedRender>;
}

/// Single-line placeholder filling at most the viewport width — used by
/// paged renderers when a page can't be decoded / rendered.
pub fn image_placeholder(text: &str, term: TermSize) -> PagedRender {
    let cols = (text.len() as u32).max(1).min(term.cols);
    PagedRender {
        lines: vec![text.to_string()],
        effective_cols: cols,
        effective_rows: 1,
        viewport_cols: cols,
        viewport_rows: 1,
    }
}

/// Build a [`TermSize`] for a paged renderer call from the live render
/// context, capping unbounded pipe-mode rows.
pub fn term_size_for(ctx_cols: usize, ctx_rows: usize) -> TermSize {
    cell_size::term_size(ctx_cols.min(u32::MAX as usize) as u32, pipe_rows(ctx_rows))
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
        for action in [Action::Next, Action::OpenSearch, Action::Back] {
            assert!(cycle_image_config(action, &mut cfg).is_none());
        }
    }
}
