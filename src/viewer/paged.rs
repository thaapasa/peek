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
use crate::types::image::scroll::{self as image_scroll, ScrollBounds};
use crate::types::image::zoom::{ZoomLevel, anchor_zoom_change};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window};
use crate::viewer::ui::slice_styled_h;
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

/// Pipe-mode walk shared by paged viewers (`PagedImageMode`,
/// `EpubReadMode`): emit each page in order, separated by a blank
/// line. The caller's `emit_page` closure picks how to render index
/// `i` and write to `out` — typically setting some `current` cursor,
/// reading from a render cache, and writing the lines.
pub(crate) fn pipe_walk_pages<F>(
    out: &mut PrintOutput,
    total: usize,
    mut emit_page: F,
) -> Result<()>
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
/// pages, `b` / `m` / `f` cycle image config, `+` / `-` / `0` / `1`..`9`
/// zoom. Per-page render cache is keyed by viewport size + image config
/// — zoom is encoded by multiplying viewport size into the cache key,
/// so two views that produce the same effective grid share a cached
/// render automatically. Mirrors
/// [`crate::viewer::modes::RenderedTextMode`] for text documents.
///
/// At zoom > 1 the renderer produces a bigger Vec<String> than the
/// viewport (zoom-aware "effective grid"); the visible viewport is
/// sliced out per render by [`slice_styled_h`] + [`slice_window`].
/// This is the "naive" zoom path — full effective grid rendered into
/// the per-page cache rather than re-rasterizing only the visible ROI.
pub(crate) struct PagedImageMode<R: PageRenderer> {
    renderer: R,
    image_config: ImageConfig,
    current: usize,
    cache: Vec<Option<CachedRender>>,
    warnings: Vec<String>,
    zoom: ZoomLevel,
    scroll_x: u32,
    scroll_y: u32,
    /// Last viewport rendered into, captured at the end of
    /// `render_window`. Read by `handle` / `scroll` to compute the
    /// effective grid bounds without re-running the renderer.
    last_viewport_cols: u32,
    last_viewport_rows: u32,
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
            zoom: ZoomLevel::one(),
            scroll_x: 0,
            scroll_y: 0,
            last_viewport_cols: 0,
            last_viewport_rows: 0,
        }
    }

    /// Cache + render the current page at the given viewport, scaled
    /// by the live zoom level. The returned slice contains the
    /// *effective grid* (viewport × zoom) — the caller slices the
    /// visible viewport out of it.
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
        let zoom_factor = self.zoom.factor();
        let scaled_width = ((width as f32 * zoom_factor).round() as usize).max(1);
        // `rows` may be `usize::MAX` (pipe path); guard the multiply.
        let scaled_rows = if rows == usize::MAX {
            rows
        } else {
            ((rows as f32 * zoom_factor).round() as usize).max(1)
        };
        let key = PageCacheKey::build(&self.image_config, scaled_width, scaled_rows, style_mode);
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

    fn effective_grid(&self, lines: &[String]) -> (u32, u32) {
        let rows = lines.len() as u32;
        let cols = lines
            .iter()
            .map(|l| crate::viewer::ui::strip_ansi_width(l) as u32)
            .max()
            .unwrap_or(0);
        (cols, rows)
    }

    /// Apply a zoom action against the last-rendered viewport. Anchors
    /// the viewport-centre cell across the change so the page point
    /// under the cursor stays put. Mirrors `ImageView::handle_zoom` —
    /// the math is duplicated rather than shared because the paged
    /// scroll state lives on this struct, not on an `ImageView`.
    fn apply_zoom(&mut self, action: Action) -> Option<Handled> {
        let new_zoom = match action {
            Action::ZoomIn => self.zoom.step_in(),
            Action::ZoomOut => self.zoom.step_out(),
            Action::ZoomReset => {
                self.zoom = ZoomLevel::one();
                self.scroll_x = 0;
                self.scroll_y = 0;
                return Some(Handled::Yes);
            }
            Action::ZoomPreset(n) => ZoomLevel::preset(n),
            _ => return None,
        };
        if new_zoom == self.zoom {
            return Some(Handled::Yes);
        }
        anchor_zoom_change(
            self.zoom.factor(),
            new_zoom.factor(),
            self.last_viewport_cols,
            self.last_viewport_rows,
            &mut self.scroll_x,
            &mut self.scroll_y,
        );
        self.zoom = new_zoom;
        Some(Handled::Yes)
    }

    fn clamp_scroll(
        &mut self,
        eff_cols: u32,
        eff_rows: u32,
        viewport_cols: u32,
        viewport_rows: u32,
    ) {
        let max_x = eff_cols.saturating_sub(viewport_cols);
        let max_y = eff_rows.saturating_sub(viewport_rows);
        self.scroll_x = self.scroll_x.min(max_x);
        self.scroll_y = self.scroll_y.min(max_y);
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

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, rows: usize) -> Result<Window> {
        let term_cols = ctx.term_cols as u32;
        let lines =
            self.ensure_rendered(ctx.term_cols, ctx.term_rows, ctx.peek_theme.style_mode)?;
        // Slice owned into vec so we can drop the cache borrow before
        // mutating self's scroll state.
        let lines: Vec<String> = lines.to_vec();
        let (eff_cols, eff_rows) = self.effective_grid(&lines);
        let viewport_cols = eff_cols.min(term_cols).max(1);
        let viewport_rows = eff_rows.min(rows.min(u32::MAX as usize) as u32).max(1);
        self.clamp_scroll(eff_cols, eff_rows, viewport_cols, viewport_rows);
        self.last_viewport_cols = viewport_cols;
        self.last_viewport_rows = viewport_rows;

        let vert = slice_window(&lines, self.scroll_y as usize, viewport_rows as usize);
        let lines: Vec<String> = if eff_cols <= term_cols && self.scroll_x == 0 {
            // No horizontal overflow — skip the SGR-aware slice.
            vert
        } else {
            vert.iter()
                .map(|l| slice_styled_h(l, self.scroll_x as usize, viewport_cols as usize))
                .collect()
        };
        Ok(Window {
            lines,
            total: eff_rows as usize,
        })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache
            .get(self.current)
            .and_then(|c| c.as_ref())
            .map(|c| c.lines.len())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        // Need cached lines + last viewport to compute bounds; bail
        // optimistically before first render.
        let Some(cached) = self.cache.get(self.current).and_then(|c| c.as_ref()) else {
            return false;
        };
        let (eff_cols, eff_rows) = self.effective_grid(&cached.lines);
        let max_x = eff_cols.saturating_sub(self.last_viewport_cols);
        let max_y = eff_rows.saturating_sub(self.last_viewport_rows);
        let page_y = self.last_viewport_rows.saturating_sub(1);
        image_scroll::apply(
            &mut self.scroll_x,
            &mut self.scroll_y,
            action,
            ScrollBounds::clamped(max_x, max_y, page_y),
        )
    }

    /// Print mode walks every page in order, separated by a blank line.
    /// Honors the cache so already-rendered pages reuse their output;
    /// the interactive view stays single-page. Forces zoom = 1× and
    /// pan = origin for the duration — pipe output is non-interactive,
    /// so the user's live zoom can't help and would only widen lines
    /// past the terminal.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let total = self.renderer.page_count();
        let saved = self.current;
        let saved_zoom = self.zoom;
        let saved_scroll = (self.scroll_x, self.scroll_y);
        self.zoom = ZoomLevel::one();
        self.scroll_x = 0;
        self.scroll_y = 0;
        let res = pipe_walk_pages(out, total, |i, out| {
            self.current = i;
            let lines =
                self.ensure_rendered(ctx.term_cols, ctx.term_rows, ctx.peek_theme.style_mode)?;
            for line in lines {
                out.write_line(line)?;
            }
            Ok(())
        });
        self.current = saved;
        self.zoom = saved_zoom;
        self.scroll_x = saved_scroll.0;
        self.scroll_y = saved_scroll.1;
        res
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        if let Some(h) = cycle_image_config(action, &mut self.image_config) {
            // Fit change invalidates the rendered grid; reset pan.
            if matches!(action, Action::CycleFitMode) {
                self.scroll_x = 0;
                self.scroll_y = 0;
            }
            return h;
        }
        if let Some(h) = self.apply_zoom(action) {
            return h;
        }
        let count = self.renderer.page_count();
        match action {
            Action::NextChapter => {
                let h = step_paged(&mut self.current, count, 1);
                if matches!(h, Handled::YesResetScroll) {
                    self.scroll_x = 0;
                    self.scroll_y = 0;
                }
                h
            }
            Action::PrevChapter => {
                let h = step_paged(&mut self.current, count, -1);
                if matches!(h, Handled::YesResetScroll) {
                    self.scroll_x = 0;
                    self.scroll_y = 0;
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
        if !self.zoom.is_one() {
            out.push((self.zoom.label(), theme.label));
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
    /// must advance scroll_x and the next render must slice from the
    /// new offset.
    #[test]
    fn paged_mode_horizontal_scroll_under_zoom() {
        use crate::info::{FileExtras, FileInfo, RenderOptions};
        use crate::theme::{PeekTheme, PeekThemeName, load_embedded_theme};
        use crate::types::binary::info::BinaryInfo;

        struct WideRenderer;
        impl PageRenderer for WideRenderer {
            fn page_count(&self) -> usize {
                1
            }
            fn render_page(
                &self,
                _idx: usize,
                _config: ImageConfig,
                _key: &PageCacheKey,
                _warnings: &mut Vec<String>,
            ) -> Result<Vec<String>> {
                // 40 rows × 160 visible cells per row — wider than the
                // 80-col terminal so horizontal pan must engage.
                Ok((0..40).map(|_| "X".repeat(160)).collect())
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
        let mut mode = PagedImageMode::new(WideRenderer, cfg);

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
            extras: FileExtras::Binary(BinaryInfo { format: None }),
        };
        let ctx = RenderCtx {
            file_info: &file_info,
            theme_name: PeekThemeName::default(),
            peek_theme: &peek_theme,
            render_opts: RenderOptions::default(),
            term_cols: 80,
            term_rows: 40,
        };

        // Populate the cache + bounds via an initial render.
        let win = mode.render_window(&ctx, 0, 40).expect("render");
        assert_eq!(win.lines.len(), 40);
        assert_eq!(mode.last_viewport_cols, 80);
        assert_eq!(mode.scroll_x, 0);

        // Right arrow: scroll_x advances by HSTEP (= 4 cells).
        assert!(Mode::scroll(&mut mode, Action::ScrollRight));
        assert_eq!(mode.scroll_x, 4);

        // Next render slices from col 4 — verifies the horizontal slice
        // path engages with scroll_x > 0.
        let win = mode.render_window(&ctx, 0, 40).expect("render");
        assert_eq!(win.lines[0].chars().filter(|&c| c == 'X').count(), 80);
    }

    /// Real-world repro: a landscape CBZ page at zoom 2× under fit=Contain
    /// produces a grid wider than the viewport, so Right arrow must
    /// advance scroll_x.
    #[test]
    fn cbz_horizontal_scroll_at_zoom_2x() {
        use crate::info::{FileExtras, FileInfo, RenderOptions};
        use crate::input::InputSource;
        use crate::theme::{PeekTheme, PeekThemeName, load_embedded_theme};
        use crate::types::binary::info::BinaryInfo;
        use crate::types::comic::CbzPageRenderer;
        use crate::types::comic::cbz;
        use crate::types::image::zoom::ZoomLevel;
        use std::path::PathBuf;

        let cbz_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-books/sample-pages.cbz");
        if !cbz_path.exists() {
            eprintln!("skip: fixture missing");
            return;
        }
        let source = InputSource::File(cbz_path);
        let pages = cbz::package::list_pages(&source).expect("list pages");
        assert!(pages.len() >= 2);
        let cfg = ImageConfig {
            mode: ImageMode::from_str("block"),
            width: 0,
            background: Background::from_str("auto"),
            margin: 0,
            style_mode: StyleMode::Plain,
            edge_density: 0.1,
            fit: FitMode::Contain,
        };
        let mut mode = PagedImageMode::new(CbzPageRenderer::new(source, pages), cfg);
        // Page 2 is landscape (1500x1000) — likeliest to overflow at zoom 2×.
        mode.current = 1;
        mode.zoom = ZoomLevel::preset(2);

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
            extras: FileExtras::Binary(BinaryInfo { format: None }),
        };
        let ctx = RenderCtx {
            file_info: &file_info,
            theme_name: PeekThemeName::default(),
            peek_theme: &peek_theme,
            render_opts: RenderOptions::default(),
            term_cols: 80,
            term_rows: 40,
        };

        let win = mode.render_window(&ctx, 0, 40).expect("render");
        let cached = mode.cache.get(1).and_then(|c| c.as_ref()).expect("cached");
        let max_line_w = cached
            .lines
            .iter()
            .map(|l| crate::viewer::ui::strip_ansi_width(l))
            .max()
            .unwrap_or(0);
        // Effective grid must overflow the 80-col viewport at zoom 2× on
        // landscape content.
        eprintln!(
            "cached lines: {}  max width: {}  last_vp: ({},{})  total: {}",
            cached.lines.len(),
            max_line_w,
            mode.last_viewport_cols,
            mode.last_viewport_rows,
            win.total
        );
        assert!(
            max_line_w > 80,
            "expected overflow at zoom 2×, got max line width {max_line_w}"
        );

        assert!(Mode::scroll(&mut mode, Action::ScrollRight));
        assert_eq!(mode.scroll_x, 4, "scroll_x must advance by HSTEP");
    }

    /// Reproduces zoom=1 + fit=FitHeight on a landscape CBZ page —
    /// the case where horizontal overflow exists without any zoom.
    /// Pressing Right must pan horizontally.
    #[test]
    fn cbz_horizontal_scroll_fit_height_zoom_one() {
        use crate::info::{FileExtras, FileInfo, RenderOptions};
        use crate::input::InputSource;
        use crate::theme::{PeekTheme, PeekThemeName, load_embedded_theme};
        use crate::types::binary::info::BinaryInfo;
        use crate::types::comic::CbzPageRenderer;
        use crate::types::comic::cbz;
        use std::path::PathBuf;

        let cbz_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-books/sample-pages.cbz");
        if !cbz_path.exists() {
            return;
        }
        let source = InputSource::File(cbz_path);
        let pages = cbz::package::list_pages(&source).expect("list pages");
        let cfg = ImageConfig {
            mode: ImageMode::from_str("block"),
            width: 0,
            background: Background::from_str("auto"),
            margin: 0,
            style_mode: StyleMode::Plain,
            edge_density: 0.1,
            fit: FitMode::FitHeight,
        };
        let mut mode = PagedImageMode::new(CbzPageRenderer::new(source, pages), cfg);
        mode.current = 1; // landscape page

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
            extras: FileExtras::Binary(BinaryInfo { format: None }),
        };
        let ctx = RenderCtx {
            file_info: &file_info,
            theme_name: PeekThemeName::default(),
            peek_theme: &peek_theme,
            render_opts: RenderOptions::default(),
            term_cols: 80,
            term_rows: 40,
        };

        let _ = mode.render_window(&ctx, 0, 40).expect("render");
        let cached = mode.cache.get(1).and_then(|c| c.as_ref()).expect("cached");
        let max_line_w = cached
            .lines
            .iter()
            .map(|l| crate::viewer::ui::strip_ansi_width(l))
            .max()
            .unwrap_or(0);
        eprintln!(
            "FitHeight zoom=1: lines={} max_w={} vp=({},{})",
            cached.lines.len(),
            max_line_w,
            mode.last_viewport_cols,
            mode.last_viewport_rows
        );

        assert!(Mode::scroll(&mut mode, Action::ScrollRight));
        eprintln!("after Right: scroll_x={}", mode.scroll_x);
        assert!(max_line_w > 80, "expected horizontal overflow");
        assert!(mode.scroll_x > 0, "Right arrow must advance scroll_x");
    }
}
