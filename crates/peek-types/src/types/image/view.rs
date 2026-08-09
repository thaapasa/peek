//! [`ImageView`] — shared image-grid scroll + config state for every
//! Mode that scrolls through a [`PreparedImage`] cell grid:
//! [`super::mode::ImageRenderMode`] (static raster / rasterized SVG),
//! [`super::animation_mode::AnimationMode`] (GIF / WebP playback), and
//! [`crate::types::svg::animation_mode::SvgAnimationMode`] (CSS
//! `@keyframes` playback).
//!
//! Holds the live `ImageConfig` (cycleable background / image-mode /
//! fit) and the `(scroll_x, scroll_y)` pan into the prepared grid.
//! Exposes the boilerplate the three Modes used to copy-paste:
//! `prepare_term` (TermSize + style_mode sync), `render_prepared`
//! (clamp pan + carve `GridWindow` + render), `pipe_snapshot` /
//! `restore` (save+restore around forced-Contain pipe path), `scroll`
//! (delegate to [`super::scroll::apply`]), `handle_config_cycle`
//! (cycle keys + reset pan on fit change).
//!
//! Each Mode embeds an [`ImageView`] and keeps the parts that genuinely
//! differ: the prep source (file / decoded-frame list / SVG keyframe
//! model), the cache strategy (single-slot / none / bounded LRU), and
//! per-Mode frame state. The shared piece is the *grid* — once a
//! `PreparedImage` exists, the same scroll / clamp / window / render
//! sequence applies regardless of where it came from.

use anyhow::Result;
use peek_theme::PeekTheme;
use syntect::highlighting::Color;

use super::pipeline::render::{self, GridWindow, PreparedImage, TermSize};
use super::pipeline::{FitMode, ImageConfig};
use super::scroll::ScrollBounds;
use super::zoom::{ZoomLevel, ZoomedView};
use super::zoom_pan::{ViewBounds, ZoomPanState};
use crate::output::PrintOutput;
use crate::viewer::cell_size;
use crate::viewer::modes::{Handled, RenderCtx, Window};
use crate::viewer::paged::cycle_image_config;
use crate::viewer::ui::Action;

/// Image-grid view state: image config + zoom/pan apparatus. Embedded
/// by every Mode that scrolls through a [`PreparedImage`].
///
/// At zoom = 1 the prepared grid drives the viewport directly. Above
/// 1 the *effective* grid is `(prep.cols × zoom, prep.rows × zoom)`
/// and `pan.scroll_x/y` are cell offsets into that effective grid;
/// rendering crops the matching pixel ROI from `prep.source` so the
/// working buffer stays viewport-sized regardless of zoom.
pub(crate) struct ImageView {
    pub config: ImageConfig,
    pub pan: ZoomPanState,
}

/// Snapshot of the fields [`ImageView::pipe_snapshot`] overrides for
/// the pipe path. Pass back to [`ImageView::restore`] to undo.
#[derive(Copy, Clone)]
pub(crate) struct ImageViewPipeSnapshot {
    fit: FitMode,
    pan: ZoomPanState,
}

impl ImageView {
    pub fn new(config: ImageConfig) -> Self {
        Self {
            config,
            pan: ZoomPanState::new(),
        }
    }

    /// Live zoom level. Exposed for renderers that bucket detail by
    /// integer zoom (SVG / font specimen).
    pub fn zoom(&self) -> ZoomLevel {
        self.pan.zoom
    }

    /// Compute scroll bounds for the live zoom level + prepared grid +
    /// terminal viewport. Use this — not `render::max_scroll` directly
    /// — anywhere that clamps pan so zoom is honoured.
    pub fn view_bounds(&self, prep: &PreparedImage, term: TermSize) -> ViewBounds {
        let zv = ZoomedView {
            base_cols: prep.cols,
            base_rows: prep.rows,
            term_cols: term.cols,
            term_rows: term.rows,
            zoom: self.pan.zoom.factor(),
        };
        let (max_x, max_y) = zv.max_scroll();
        let (viewport_cols, viewport_rows) = zv.viewport();
        ViewBounds {
            max_x,
            max_y,
            viewport_cols,
            viewport_rows,
        }
    }

    /// Build a [`TermSize`] from the live render context and sync the
    /// cyclable `style_mode` onto the held config. Call once at the
    /// top of `render_window` before preparing the image.
    pub fn prepare_term(&mut self, ctx: &RenderCtx) -> TermSize {
        self.config.style_mode = ctx.peek_theme.style_mode;
        cell_size::term_size(
            ctx.term_cols.min(u32::MAX as usize) as u32,
            ctx.term_rows.min(u32::MAX as usize) as u32,
        )
    }

    /// Clamp the held pan offsets to the effective grid + viewport,
    /// carve the visible sub-rectangle, and render. At zoom = 1 takes
    /// the fast path through the cached resized buffer; at zoom > 1
    /// crops the source pixel ROI matching the viewport so memory
    /// stays viewport-sized.
    ///
    /// `total` in the returned [`Window`] is the effective row count
    /// (zoom-aware) so the status line's scroll math has the right
    /// denominator at every zoom level.
    pub fn render_prepared(&mut self, prep: &PreparedImage, term: TermSize) -> Window {
        if self.pan.zoom.is_one() {
            let (max_x, max_y) = render::max_scroll(prep.cols, prep.rows, term.cols, term.rows);
            self.pan.scroll_x = self.pan.scroll_x.min(max_x);
            self.pan.scroll_y = self.pan.scroll_y.min(max_y);
            let visible_cols = prep.cols.min(term.cols);
            let visible_rows = prep.rows.min(term.rows);
            let window = GridWindow {
                col_start: self.pan.scroll_x,
                col_end: self.pan.scroll_x + visible_cols,
                row_start: self.pan.scroll_y,
                row_end: self.pan.scroll_y + visible_rows,
            };
            let lines = render::render_prepared(prep, &self.config, window);
            return Window {
                lines,
                total: prep.rows as usize,
            };
        }
        let result = render::render_prepared_zoomed(
            prep,
            &self.config,
            term,
            self.pan.zoom.factor(),
            self.pan.scroll_x,
            self.pan.scroll_y,
        );
        let max_x = result.effective_cols.saturating_sub(result.viewport_cols);
        let max_y = result.effective_rows.saturating_sub(result.viewport_rows);
        self.pan.scroll_x = self.pan.scroll_x.min(max_x);
        self.pan.scroll_y = self.pan.scroll_y.min(max_y);
        Window {
            lines: result.lines,
            total: result.effective_rows as usize,
        }
    }

    /// Save the live fit + pan + zoom, then force `Contain` + reset
    /// pan + reset zoom for the pipe / `--print` path (unbounded rows
    /// make `FitHeight` meaningless and a zoomed viewport doesn't make
    /// sense without an interactive terminal to pan). Caller then
    /// renders and writes lines, then passes the returned snapshot
    /// back to [`restore`](Self::restore).
    pub fn pipe_snapshot(&mut self) -> ImageViewPipeSnapshot {
        let snap = ImageViewPipeSnapshot {
            fit: self.config.fit,
            pan: self.pan,
        };
        self.config.fit = FitMode::Contain;
        self.pan = ZoomPanState::new();
        snap
    }

    /// Undo [`pipe_snapshot`](Self::pipe_snapshot).
    pub fn restore(&mut self, snap: ImageViewPipeSnapshot) {
        self.config.fit = snap.fit;
        self.pan = snap.pan;
    }

    /// Write the rendered window line-by-line. Convenience wrapper —
    /// every Mode's pipe path does exactly this.
    pub fn write_lines(out: &mut PrintOutput, window: Window) -> Result<()> {
        for line in window.lines {
            out.write_line(&line)?;
        }
        Ok(())
    }

    /// Pass a scroll action to the shared 2D scroll handler with the
    /// caller-supplied bounds (clamped to live prep dims, or
    /// unbounded for per-tick decode paths).
    pub fn scroll(&mut self, action: Action, bounds: ScrollBounds) -> bool {
        self.pan.scroll(action, bounds)
    }

    /// Apply an image-config cycle key (`b` / `m` / `f` family) and
    /// reset the pan on a fit-mode change (the old offset is
    /// meaningless against the new grid). Returns `Some(Handled)`
    /// when the key matched, `None` to let the caller's own `match`
    /// continue.
    pub fn handle_config_cycle(&mut self, action: Action) -> Option<Handled> {
        let h = cycle_image_config(action, &mut self.config)?;
        if action == Action::CycleFitMode {
            self.pan.reset_pan();
        }
        Some(h)
    }

    /// Apply a zoom action against the currently-displayed grid +
    /// viewport — delegates to [`ZoomPanState::handle_zoom`].
    pub fn handle_zoom(&mut self, action: Action, bounds: ViewBounds) -> Option<Handled> {
        self.pan.handle_zoom(action, bounds)
    }

    /// `[mode.label, fit.label, zoom.label?]` — the status segments
    /// common to every image view. Zoom appears only when > 1× so the
    /// default view stays uncluttered. Animation Modes append a frame
    /// counter via
    /// [`super::anim_frame::AnimFrameState::status_segment`].
    pub fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let mut out = vec![
            (self.config.mode.label().to_string(), theme.label),
            (self.config.fit.label().to_string(), theme.label),
        ];
        if !self.pan.zoom.is_one() {
            out.push((self.pan.zoom.label(), theme.label));
        }
        out
    }
}
