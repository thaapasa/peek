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
//! for text documents. The renderer owns whatever per-page caching its
//! source needs (decoded source bitmap for CBZ, rasterized effective
//! grid for PDF) and returns the viewport-sized ASCII for the current
//! zoom/scroll state; [`PagedImageMode<R>`] handles navigation, zoom,
//! pan, and the `Mode` impl itself but no longer caches rendered
//! output of its own. EPUB stays separate by design: chapter search
//! and cover-style inline image rendering would have to be lifted into
//! [`PagedImageMode<R>`] as generic concerns first — neither belongs
//! in PDF / CBZ. Prior `/checkup` rounds decided that's not worth
//! doing for one consumer;
//! `the EPUB read mode` keeps its
//! own `Mode` impl reusing the navigation / config-cycle building
//! blocks here ([`render_cached`], [`step_paged`], [`cycle_image_config`],
//! [`PageCacheKey`]).

use anyhow::Result;
use syntect::highlighting::Color;

use crate::output::PrintOutput;
use crate::theme::{PeekTheme, StyleMode};
use crate::viewer::cell_size;
use crate::viewer::image_render::{
    Background, FitMode, ImageConfig, ImageMode, ScrollBounds, TermSize, ViewBounds, ZoomLevel,
    ZoomPanState,
};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::viewer::ui::{Action, HelpEntry};

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

/// Mode-local help entries for [`PagedImageMode`]: page navigation plus
/// the shared image-config block plus zoom.
///
/// `ScrollLeft` / `ScrollRight` are listed explicitly even though the
/// scroll machinery handles them — dispatch goes through this slice to
/// find a key binding, and the global slice only carries vertical
/// scroll. Without this entry Left/Right never reach the mode.
const EXTRA_ACTIONS: &[HelpEntry] = &[
    (
        &[Action::NextChapter, Action::PrevChapter],
        "Next / previous page",
    ),
    CYCLE_BACKGROUND_HELP,
    CYCLE_IMAGE_MODE_HELP,
    CYCLE_FIT_HELP,
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Pan left / right (when zoomed or fit=FitHeight)",
    ),
    (&[Action::ZoomIn, Action::ZoomOut], "Zoom in / out"),
    (&[Action::ZoomReset], "Reset zoom to 1×"),
    (
        &[
            Action::ZoomPreset(1),
            Action::ZoomPreset(2),
            Action::ZoomPreset(3),
            Action::ZoomPreset(4),
            Action::ZoomPreset(5),
            Action::ZoomPreset(6),
            Action::ZoomPreset(7),
            Action::ZoomPreset(8),
            Action::ZoomPreset(9),
        ],
        "Zoom 1×–9×",
    ),
];

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

/// Paged-image read mode generic over its [`PageRenderer`].
///
/// Shows one page at a time through the image pipeline; `n` / `p` step
/// pages, `b` / `m` / `f` cycle image config, `+` / `-` / `0` / `1`..`9`
/// zoom. The renderer owns whatever per-page caching makes sense for
/// its source (decoded source bitmap for CBZ, rasterized effective
/// grid for PDF) and returns the visible viewport's ASCII for the
/// current zoom + pan state — this mode no longer holds a rendered-
/// output cache of its own, so memory stays bounded by viewport
/// rather than effective grid (`viewport × zoom²`).
pub struct PagedImageMode<R: PageRenderer> {
    renderer: R,
    image_config: ImageConfig,
    /// Tab label. Defaults to "Read" for single-view paged documents
    /// (PDF / CBZ); EPS overrides it to distinguish its "Preview" and
    /// "Render" image tabs.
    label: &'static str,
    current: usize,
    warnings: Vec<String>,
    pan: ZoomPanState,
    /// Last viewport rendered into, captured at the end of
    /// `render_window`. Read by `handle` / `scroll` to compute scroll
    /// bounds and zoom anchoring without re-running the renderer.
    last_viewport_cols: u32,
    last_viewport_rows: u32,
    /// Last effective-grid dims from the renderer. Used by `scroll`
    /// to clamp pan bounds without invoking the renderer.
    last_effective_cols: u32,
    last_effective_rows: u32,
}

impl<R: PageRenderer> PagedImageMode<R> {
    pub fn new(renderer: R, image_config: ImageConfig) -> Self {
        Self::with_label(renderer, image_config, "Read")
    }

    /// Like [`Self::new`] but with a caller-supplied tab label.
    pub fn with_label(renderer: R, image_config: ImageConfig, label: &'static str) -> Self {
        Self {
            renderer,
            image_config,
            label,
            current: 0,
            warnings: Vec::new(),
            pan: ZoomPanState::new(),
            last_viewport_cols: 0,
            last_viewport_rows: 0,
            last_effective_cols: 0,
            last_effective_rows: 0,
        }
    }
}

impl<R: PageRenderer> Mode for PagedImageMode<R> {
    fn id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn label(&self) -> &str {
        self.label
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, _rows: usize) -> Result<Window> {
        if self.renderer.page_count() == 0 {
            self.last_viewport_cols = 0;
            self.last_viewport_rows = 0;
            self.last_effective_cols = 0;
            self.last_effective_rows = 0;
            return Ok(Window {
                lines: Vec::new(),
                total: 0,
            });
        }
        let args = RenderArgs {
            term: term_size_for(ctx.term_cols, ctx.term_rows),
            zoom: self.pan.zoom,
            scroll_x: self.pan.scroll_x,
            scroll_y: self.pan.scroll_y,
            style_mode: ctx.peek_theme.style_mode,
        };
        let render =
            self.renderer
                .render_page(self.current, self.image_config, args, &mut self.warnings)?;
        self.last_viewport_cols = render.viewport_cols;
        self.last_viewport_rows = render.viewport_rows;
        self.last_effective_cols = render.effective_cols;
        self.last_effective_rows = render.effective_rows;
        // Clamp scroll against the freshly observed effective grid so
        // future redraws / scroll calls start from a valid origin.
        let max_x = render.effective_cols.saturating_sub(render.viewport_cols);
        let max_y = render.effective_rows.saturating_sub(render.viewport_rows);
        self.pan.scroll_x = self.pan.scroll_x.min(max_x);
        self.pan.scroll_y = self.pan.scroll_y.min(max_y);
        Ok(Window {
            lines: render.lines,
            total: render.effective_rows as usize,
        })
    }

    fn total_lines(&self) -> Option<usize> {
        if self.last_effective_rows == 0 {
            None
        } else {
            Some(self.last_effective_rows as usize)
        }
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        // Bail optimistically before the first render — without it we
        // have no effective-grid or viewport dims to clamp against.
        if self.last_viewport_cols == 0 || self.last_viewport_rows == 0 {
            return false;
        }
        let max_x = self
            .last_effective_cols
            .saturating_sub(self.last_viewport_cols);
        let max_y = self
            .last_effective_rows
            .saturating_sub(self.last_viewport_rows);
        let page_y = self.last_viewport_rows.saturating_sub(1);
        self.pan
            .scroll(action, ScrollBounds::clamped(max_x, max_y, page_y))
    }

    /// Print mode walks every page in order, separated by a blank line.
    /// Honors the cache so already-rendered pages reuse their output;
    /// the interactive view stays single-page. Forces zoom = 1× and
    /// pan = origin for the duration — pipe output is non-interactive,
    /// so the user's live zoom can't help and would only widen lines
    /// past the terminal.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let total = self.renderer.page_count();
        let saved_current = self.current;
        let saved_pan = std::mem::replace(&mut self.pan, ZoomPanState::new());
        let args = RenderArgs {
            term: term_size_for(ctx.term_cols, ctx.term_rows),
            zoom: self.pan.zoom,
            scroll_x: 0,
            scroll_y: 0,
            style_mode: ctx.peek_theme.style_mode,
        };
        let renderer = &self.renderer;
        let config = self.image_config;
        let warnings = &mut self.warnings;
        let res = pipe_walk_pages(out, total, |i, out| {
            let render = renderer.render_page(i, config, args, warnings)?;
            for line in &render.lines {
                out.write_line(line)?;
            }
            Ok(())
        });
        self.current = saved_current;
        self.pan = saved_pan;
        res
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        if let Some(h) = cycle_image_config(action, &mut self.image_config) {
            // Fit change invalidates the rendered grid; reset pan.
            if matches!(action, Action::CycleFitMode) {
                self.pan.reset_pan();
            }
            return h;
        }
        let zoom_bounds = ViewBounds {
            max_x: self
                .last_effective_cols
                .saturating_sub(self.last_viewport_cols),
            max_y: self
                .last_effective_rows
                .saturating_sub(self.last_viewport_rows),
            viewport_cols: self.last_viewport_cols,
            viewport_rows: self.last_viewport_rows,
        };
        if let Some(h) = self.pan.handle_zoom(action, zoom_bounds) {
            return h;
        }
        let count = self.renderer.page_count();
        match action {
            Action::NextChapter => {
                let h = step_paged(&mut self.current, count, 1);
                if matches!(h, Handled::YesResetScroll) {
                    self.pan.reset_pan();
                }
                h
            }
            Action::PrevChapter => {
                let h = step_paged(&mut self.current, count, -1);
                if matches!(h, Handled::YesResetScroll) {
                    self.pan.reset_pan();
                }
                h
            }
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let count = self.renderer.page_count();
        if count == 0 {
            return Vec::new();
        }
        let mut out = vec![(format!("page {}/{}", self.current + 1, count), theme.muted)];
        if !self.pan.zoom.is_one() {
            out.push((self.pan.zoom.label(), theme.label));
        }
        out
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

    /// Regression: PagedImageMode must advertise `ScrollLeft` /
    /// `ScrollRight` in its `EXTRA_ACTIONS` slice so the global key
    /// dispatcher can match Left/Right and route them to the mode.
    /// `GLOBAL_ACTIONS` only carries vertical scroll; horizontal pan
    /// is opt-in per mode (same pattern as ImageRenderMode +
    /// AnimationMode + SvgAnimationMode + SpecimenMode).
    #[test]
    fn paged_extra_actions_carries_horizontal_scroll() {
        let actions: Vec<Action> = EXTRA_ACTIONS
            .iter()
            .flat_map(|(keys, _)| keys.iter().copied())
            .collect();
        assert!(
            actions.contains(&Action::ScrollLeft),
            "EXTRA_ACTIONS must include ScrollLeft so Left routes through dispatch"
        );
        assert!(
            actions.contains(&Action::ScrollRight),
            "EXTRA_ACTIONS must include ScrollRight so Right routes through dispatch"
        );
    }

    /// Regression: horizontal scroll under zoom on PagedImageMode. With
    /// a wide effective grid (e.g. zoomed CBZ page), pressing `Right`
    /// must advance scroll_x and the renderer must see the new offset
    /// on the next call.
    #[test]
    fn paged_mode_horizontal_scroll_under_zoom() {
        use crate::info::{FileInfo, NoExtras, RenderOptions};
        use crate::theme::{PeekTheme, PeekThemeName, load_embedded_theme};
        use std::cell::Cell;

        struct WideRenderer {
            last_scroll_x: Cell<u32>,
        }
        impl PageRenderer for WideRenderer {
            fn page_count(&self) -> usize {
                1
            }
            fn render_page(
                &self,
                _idx: usize,
                _config: ImageConfig,
                args: RenderArgs,
                _warnings: &mut Vec<String>,
            ) -> Result<PagedRender> {
                self.last_scroll_x.set(args.scroll_x);
                // Effective grid 160×40, viewport (clamped to 80-col
                // terminal) = 80×40. Renderer fills the viewport with
                // 'X' so the test can spot-check shape.
                Ok(PagedRender {
                    lines: (0..40).map(|_| "X".repeat(80)).collect(),
                    effective_cols: 160,
                    effective_rows: 40,
                    viewport_cols: 80,
                    viewport_rows: 40,
                })
            }
        }

        let cfg = ImageConfig {
            mode: ImageMode::from_str("block"),
            width: 0,
            background: Background::from_str("auto"),
            margin: 0,
            style_mode: StyleMode::Plain,
            edge_density: 0.1,
            fit: FitMode::Contain,
        };
        let renderer = WideRenderer {
            last_scroll_x: Cell::new(0),
        };
        let mut mode = PagedImageMode::new(renderer, cfg);

        let syntect = load_embedded_theme(PeekThemeName::default().tmtheme_source());
        let peek_theme = PeekTheme::from_syntect(&syntect);
        let file_info = FileInfo {
            file_name: String::new(),
            path: String::new(),
            size_bytes: 0,
            mimes: Vec::new(),
            warnings: Vec::new(),
            modified: None,
            created: None,
            permissions: None,
            compression: None,
            extras: Box::new(NoExtras),
        };
        let ctx = RenderCtx {
            file_info: &file_info,
            theme_name: PeekThemeName::default(),
            peek_theme: &peek_theme,
            render_opts: RenderOptions::default(),
            term_cols: 80,
            term_rows: 40,
        };

        // Initial render populates the effective-grid + viewport bounds
        // on the mode so scroll has something to clamp against.
        let win = mode.render_window(&ctx, 0, 40).expect("render");
        assert_eq!(win.lines.len(), 40);
        assert_eq!(mode.last_viewport_cols, 80);
        assert_eq!(mode.last_effective_cols, 160);
        assert_eq!(mode.pan.scroll_x, 0);
        assert_eq!(mode.renderer.last_scroll_x.get(), 0);

        // Right arrow: scroll_x advances by HSTEP (= 4 cells).
        assert!(Mode::scroll(&mut mode, Action::ScrollRight));
        assert_eq!(mode.pan.scroll_x, 4);

        // Next render hands the updated scroll_x to the renderer so the
        // ROI path picks up the pan.
        let _ = mode.render_window(&ctx, 0, 40).expect("render");
        assert_eq!(mode.renderer.last_scroll_x.get(), 4);
    }
}
