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
use syntect::highlighting::Color;

use super::pipeline::render::{self, GridWindow, PreparedImage, TermSize};
use super::pipeline::{FitMode, ImageConfig};
use super::scroll::{self, ScrollBounds};
use super::zoom::ZoomLevel;
use crate::output::PrintOutput;
use crate::theme::PeekTheme;
use crate::viewer::cell_size;
use crate::viewer::modes::{Handled, RenderCtx, Window};
use crate::viewer::paged::cycle_image_config;
use crate::viewer::ui::Action;

/// Image-grid view state: image config + 2D pan offset + zoom. Embedded
/// by every Mode that scrolls through a [`PreparedImage`].
///
/// At zoom = 1 the prepared grid drives the viewport directly. Above
/// 1 the *effective* grid is `(prep.cols × zoom, prep.rows × zoom)`
/// and `(scroll_x, scroll_y)` are cell offsets into that effective
/// grid; rendering crops the matching pixel ROI from `prep.source` so
/// the working buffer stays viewport-sized regardless of zoom.
pub(crate) struct ImageView {
    pub config: ImageConfig,
    pub scroll_x: u32,
    pub scroll_y: u32,
    pub zoom: ZoomLevel,
}

/// Snapshot of the fields [`ImageView::pipe_snapshot`] overrides for
/// the pipe path. Pass back to [`ImageView::restore`] to undo.
#[derive(Copy, Clone)]
pub(crate) struct ImageViewPipeSnapshot {
    fit: FitMode,
    scroll_x: u32,
    scroll_y: u32,
    zoom: ZoomLevel,
}

/// Scroll bounds suitable for the active view: the effective grid
/// minus the terminal-clamped viewport, on each axis. Used by every
/// caller that needs to clamp pan after a scroll action so zoom > 1
/// pan ranges respect the larger effective grid.
#[derive(Copy, Clone, Debug)]
pub(crate) struct ViewBounds {
    pub max_x: u32,
    pub max_y: u32,
    pub viewport_cols: u32,
    pub viewport_rows: u32,
}

impl ImageView {
    pub fn new(config: ImageConfig) -> Self {
        Self {
            config,
            scroll_x: 0,
            scroll_y: 0,
            zoom: ZoomLevel::one(),
        }
    }

    /// Compute scroll bounds for the live zoom level + prepared grid +
    /// terminal viewport. Use this — not `render::max_scroll` directly
    /// — anywhere that clamps pan so zoom is honoured.
    pub fn view_bounds(&self, prep: &PreparedImage, term: TermSize) -> ViewBounds {
        let zoom = self.zoom.factor();
        let effective_cols = ((prep.cols as f32 * zoom).round() as u32).max(1);
        let effective_rows = ((prep.rows as f32 * zoom).round() as u32).max(1);
        let viewport_cols = effective_cols.min(term.cols).max(1);
        let viewport_rows = effective_rows.min(term.rows).max(1);
        ViewBounds {
            max_x: effective_cols.saturating_sub(viewport_cols),
            max_y: effective_rows.saturating_sub(viewport_rows),
            viewport_cols,
            viewport_rows,
        }
    }

    /// Build a [`TermSize`] from the live render context and sync the
    /// cyclable `style_mode` onto the held config. Call once at the
    /// top of `render_window` before preparing the image.
    pub fn prepare_term(&mut self, ctx: &RenderCtx) -> TermSize {
        self.config.style_mode = ctx.peek_theme.style_mode;
        TermSize {
            cols: ctx.term_cols.min(u32::MAX as usize) as u32,
            rows: ctx.term_rows.min(u32::MAX as usize) as u32,
            cell_h_over_w: cell_size::cell_aspect_h_over_w(),
        }
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
        if self.zoom.is_one() {
            let (max_x, max_y) = render::max_scroll(prep.cols, prep.rows, term.cols, term.rows);
            self.scroll_x = self.scroll_x.min(max_x);
            self.scroll_y = self.scroll_y.min(max_y);
            let visible_cols = prep.cols.min(term.cols);
            let visible_rows = prep.rows.min(term.rows);
            let window = GridWindow {
                col_start: self.scroll_x,
                col_end: self.scroll_x + visible_cols,
                row_start: self.scroll_y,
                row_end: self.scroll_y + visible_rows,
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
            self.zoom.factor(),
            self.scroll_x,
            self.scroll_y,
        );
        let max_x = result.effective_cols.saturating_sub(result.viewport_cols);
        let max_y = result.effective_rows.saturating_sub(result.viewport_rows);
        self.scroll_x = self.scroll_x.min(max_x);
        self.scroll_y = self.scroll_y.min(max_y);
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
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            zoom: self.zoom,
        };
        self.config.fit = FitMode::Contain;
        self.scroll_x = 0;
        self.scroll_y = 0;
        self.zoom = ZoomLevel::one();
        snap
    }

    /// Undo [`pipe_snapshot`](Self::pipe_snapshot).
    pub fn restore(&mut self, snap: ImageViewPipeSnapshot) {
        self.config.fit = snap.fit;
        self.scroll_x = snap.scroll_x;
        self.scroll_y = snap.scroll_y;
        self.zoom = snap.zoom;
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
        scroll::apply(&mut self.scroll_x, &mut self.scroll_y, action, bounds)
    }

    /// Apply an image-config cycle key (`b` / `m` / `f` family) and
    /// reset the pan on a fit-mode change (the old offset is
    /// meaningless against the new grid). Returns `Some(Handled)`
    /// when the key matched, `None` to let the caller's own `match`
    /// continue.
    pub fn handle_config_cycle(&mut self, action: Action) -> Option<Handled> {
        let h = cycle_image_config(action, &mut self.config)?;
        if action == Action::CycleFitMode {
            self.scroll_x = 0;
            self.scroll_y = 0;
        }
        Some(h)
    }

    /// Apply a zoom action (`+` / `-` / `0` / `1`..`9`) against the
    /// currently-displayed grid + viewport. `bounds` describes the
    /// *current* effective grid the user is looking at — only the
    /// viewport dimensions are read, so the pre-zoom anchor math has
    /// the viewport centre to work with. Returns `Some(Handled::Yes)`
    /// when the action matched, `None` to bubble to the caller's own
    /// match.
    ///
    /// Anchor rule: after zoom, the pixel that was under the viewport
    /// centre stays under the viewport centre (clamped to the new
    /// effective grid's edges). `ZoomReset` always sends scroll to
    /// the origin regardless of the prior pan.
    pub fn handle_zoom(&mut self, action: Action, bounds: ViewBounds) -> Option<Handled> {
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
        let old_zoom = self.zoom.factor();
        let new_zoom_f = new_zoom.factor();
        let half_w = bounds.viewport_cols as f32 / 2.0;
        let half_h = bounds.viewport_rows as f32 / 2.0;
        let centre_x = (self.scroll_x as f32 + half_w) * (new_zoom_f / old_zoom);
        let centre_y = (self.scroll_y as f32 + half_h) * (new_zoom_f / old_zoom);
        self.zoom = new_zoom;
        self.scroll_x = (centre_x - half_w).max(0.0).round() as u32;
        self.scroll_y = (centre_y - half_h).max(0.0).round() as u32;
        Some(Handled::Yes)
    }

    #[cfg(test)]
    pub(crate) fn set_zoom_for_test(&mut self, zoom: ZoomLevel) {
        self.zoom = zoom;
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
        if !self.zoom.is_one() {
            out.push((self.zoom.label(), theme.label));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::StyleMode;
    use crate::types::image::pipeline::{Background, FitMode, ImageMode};

    fn default_config() -> ImageConfig {
        ImageConfig {
            mode: ImageMode::Block,
            width: 0,
            background: Background::Auto,
            margin: 0,
            style_mode: StyleMode::Plain,
            edge_density: 0.10,
            fit: FitMode::Contain,
        }
    }

    fn bounds(viewport_cols: u32, viewport_rows: u32) -> ViewBounds {
        ViewBounds {
            max_x: 0,
            max_y: 0,
            viewport_cols,
            viewport_rows,
        }
    }

    #[test]
    fn zoom_in_keeps_viewport_centre_pixel_fixed() {
        // Viewport 80×24, scroll at origin. After zoom 1→1.25 the
        // pixel at the viewport centre (col 40, row 12) projects to
        // 50, 15 in the new effective grid. New scroll = centre − half
        // = (10, 3).
        let mut v = ImageView::new(default_config());
        v.handle_zoom(Action::ZoomIn, bounds(80, 24));
        assert_eq!((v.scroll_x, v.scroll_y), (10, 3));
        assert!((v.zoom.factor() - 1.25).abs() < 1e-3);
    }

    #[test]
    fn zoom_out_after_zoom_in_returns_to_origin() {
        let mut v = ImageView::new(default_config());
        v.handle_zoom(Action::ZoomIn, bounds(80, 24));
        v.handle_zoom(Action::ZoomOut, bounds(80, 24));
        assert!(v.zoom.is_one());
        assert_eq!((v.scroll_x, v.scroll_y), (0, 0));
    }

    #[test]
    fn zoom_reset_clears_scroll_regardless_of_pan() {
        let mut v = ImageView::new(default_config());
        v.set_zoom_for_test(ZoomLevel::new(4.0));
        v.scroll_x = 100;
        v.scroll_y = 50;
        v.handle_zoom(Action::ZoomReset, bounds(80, 24));
        assert!(v.zoom.is_one());
        assert_eq!((v.scroll_x, v.scroll_y), (0, 0));
    }

    #[test]
    fn zoom_preset_jumps_to_integer_zoom() {
        let mut v = ImageView::new(default_config());
        v.handle_zoom(Action::ZoomPreset(3), bounds(80, 24));
        assert_eq!(v.zoom.factor(), 3.0);
    }

    #[test]
    fn zoom_passes_through_non_zoom_actions() {
        let mut v = ImageView::new(default_config());
        let h = v.handle_zoom(Action::CycleFitMode, bounds(80, 24));
        assert!(h.is_none());
    }
}
