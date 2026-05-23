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
use crate::output::PrintOutput;
use crate::theme::PeekTheme;
use crate::viewer::cell_size;
use crate::viewer::modes::{Handled, RenderCtx, Window};
use crate::viewer::paged::cycle_image_config;
use crate::viewer::ui::Action;

/// Image-grid view state: image config + 2D pan offset. Embedded by
/// every Mode that scrolls through a [`PreparedImage`].
pub(crate) struct ImageView {
    pub config: ImageConfig,
    pub scroll_x: u32,
    pub scroll_y: u32,
}

/// Snapshot of the fields [`ImageView::pipe_snapshot`] overrides for
/// the pipe path. Pass back to [`ImageView::restore`] to undo.
#[derive(Copy, Clone)]
pub(crate) struct ImageViewPipeSnapshot {
    fit: FitMode,
    scroll_x: u32,
    scroll_y: u32,
}

impl ImageView {
    pub fn new(config: ImageConfig) -> Self {
        Self {
            config,
            scroll_x: 0,
            scroll_y: 0,
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

    /// Clamp the held pan offsets to the prepared grid + viewport,
    /// carve the visible sub-rectangle, and render. `total` in the
    /// returned [`Window`] is the full prepared row count so the
    /// status line's scroll math has the right denominator.
    pub fn render_prepared(&mut self, prep: &PreparedImage, term: TermSize) -> Window {
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
        Window {
            lines,
            total: prep.rows as usize,
        }
    }

    /// Save the live fit + pan, then force `Contain` + reset pan for
    /// the pipe / `--print` path (unbounded rows make `FitHeight`
    /// meaningless and `FitWidth` reduces to `Contain` anyway). Caller
    /// then renders and writes lines, then passes the returned
    /// snapshot back to [`restore`](Self::restore).
    pub fn pipe_snapshot(&mut self) -> ImageViewPipeSnapshot {
        let snap = ImageViewPipeSnapshot {
            fit: self.config.fit,
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
        };
        self.config.fit = FitMode::Contain;
        self.scroll_x = 0;
        self.scroll_y = 0;
        snap
    }

    /// Undo [`pipe_snapshot`](Self::pipe_snapshot).
    pub fn restore(&mut self, snap: ImageViewPipeSnapshot) {
        self.config.fit = snap.fit;
        self.scroll_x = snap.scroll_x;
        self.scroll_y = snap.scroll_y;
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

    /// `[mode.label, fit.label]` — the two-element status common to
    /// every image view. Animation Modes append a frame counter via
    /// [`super::anim_frame::AnimFrameState::status_segment`].
    pub fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        vec![
            (self.config.mode.label().to_string(), theme.label),
            (self.config.fit.label().to_string(), theme.label),
        ]
    }
}
