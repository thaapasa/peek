//! Animated SVG view: CSS `@keyframes` driven playback. Mirrors
//! [`crate::types::image::animation_mode::AnimationMode`] (GIF / WebP)
//! for keybindings, scroll, and status, but rasterizes each frame on
//! demand from a parsed [`AnimatedSvg`] model rather than holding
//! pre-decoded pixel buffers. A bounded LRU caches recently composited
//! frames keyed by `(frame_idx, grid_cols, grid_rows)` so playback
//! after one full loop becomes free, and switching fit / mode
//! invalidates only the entries whose grid no longer matches.
//!
//! **Not merged into a shared `AnimatedView<FrameSource>` shell with
//! `AnimationMode`.** The LRU cache here is load-bearing — rasterizing
//! an SVG frame is two orders of magnitude more expensive than walking
//! a pre-decoded pixel buffer, so caching has to live close to the
//! prep step. `AnimationMode` has no cache for the opposite reason
//! (per render is already cheap). A shared shell would either erase
//! one strategy or fan-through an enum that loses the type-level
//! distinction. Prior `/checkup` rounds decided the duplicated Mode
//! body (~80 lines) is cheaper than the abstraction.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use peek_theme::PeekTheme;
use syntect::highlighting::Color;

use crate::types::image::anim_frame::AnimFrameState;
use crate::types::image::pipeline::render::{self, PreparedImage, TermSize};
use crate::types::image::pipeline::svg_anim::{self, AnimatedSvg};
use crate::types::image::pipeline::{FitMode, ImageConfig};
use crate::types::image::scroll::ScrollBounds;
use crate::types::image::view::ImageView;
use crate::types::image::zoom::integer_bucket;
use crate::viewer::image_render::ZOOM_PRESET_HELP;
use crate::viewer::modes::{ExtractTarget, Handled, Mode, ModeId, RenderCtx, Window};
use crate::viewer::paged::{CYCLE_BACKGROUND_HELP, CYCLE_FIT_HELP, CYCLE_IMAGE_MODE_HELP};
use crate::viewer::ui::{Action, HelpEntry};

/// Maximum number of (frame, grid) prepared images held in memory.
const FRAME_CACHE: usize = 64;

#[derive(Copy, Clone, PartialEq, Eq)]
struct CacheKey {
    frame_idx: u32,
    cols: u32,
    rows: u32,
    margin: u32,
    ascii: bool,
    fit: FitMode,
    /// Zoom bucket the frame's source bitmap was rasterised for. A
    /// zoom-up crossing into the next integer bucket invalidates the
    /// entry (cache miss → re-rasterise at higher detail); zoom-down
    /// leaves the higher-bucket entry intact so a subsequent
    /// zoom-back-up is a cache hit.
    zoom_bucket: u32,
}

pub(crate) struct SvgAnimationMode {
    model: Arc<AnimatedSvg>,
    anim: AnimFrameState,
    view: ImageView,
    /// Bounded LRU of prepared frames. Fresh entries push to the back;
    /// the front evicts when full.
    cache: VecDeque<(CacheKey, PreparedImage)>,
    /// Last terminal size seen by `render_window`. Used by `scroll` to
    /// clamp authoritatively against the live grid + viewport.
    last_term: Option<TermSize>,
}

const SVG_ANIM_ACTIONS: &[HelpEntry] = &[
    (&[Action::PlayPause], "Play / pause"),
    (&[Action::Next, Action::Prev], "Next / previous frame"),
    (&[Action::Extract], "Extract current frame as PNG"),
    CYCLE_BACKGROUND_HELP,
    CYCLE_IMAGE_MODE_HELP,
    CYCLE_FIT_HELP,
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Scroll left / right (FitHeight)",
    ),
    (&[Action::ZoomIn, Action::ZoomOut], "Zoom in / out"),
    (&[Action::ZoomReset], "Reset zoom to 1×"),
    ZOOM_PRESET_HELP,
];

impl SvgAnimationMode {
    pub(crate) fn new(model: AnimatedSvg, config: ImageConfig) -> Self {
        assert!(
            !model.frames.is_empty(),
            "SvgAnimationMode requires \u{2265}1 frame"
        );
        Self {
            model: Arc::new(model),
            anim: AnimFrameState::new(),
            view: ImageView::new(config),
            cache: VecDeque::with_capacity(FRAME_CACHE),
            last_term: None,
        }
    }

    fn prepare_current(&mut self, term: TermSize) -> Result<&PreparedImage> {
        // Probe with a dry-run prepare to learn (cols, rows) for the
        // cache key — they depend on term + fit + margin + svg dims.
        // Cheap: a few arithmetic ops, no rasterization.
        let (probe_cols, probe_rows) = render::compute_grid(
            self.model.width_px + self.view.config.margin * 2,
            self.model.height_px + self.view.config.margin * 2,
            term,
            self.view.config.width,
            self.view.config.fit,
        );
        let zoom_bucket = integer_bucket(self.view.zoom().factor());
        let key = CacheKey {
            frame_idx: self.anim.current as u32,
            cols: probe_cols,
            rows: probe_rows,
            margin: self.view.config.margin,
            ascii: matches!(
                self.view.config.mode,
                crate::types::image::pipeline::ImageMode::Ascii
            ),
            fit: self.view.config.fit,
            zoom_bucket,
        };

        if let Some(pos) = self.cache.iter().position(|(k, _)| *k == key) {
            // Move to back (mark MRU) by removing + re-inserting.
            if let Some(entry) = self.cache.remove(pos) {
                self.cache.push_back(entry);
            }
        } else {
            let svg_text = svg_anim::render_frame(&self.model, self.anim.current);
            let prep = render::prepare_svg_bytes(
                svg_text.as_bytes(),
                self.model.width_px,
                self.model.height_px,
                &self.view.config,
                term,
                zoom_bucket,
            )?;

            if self.cache.len() == FRAME_CACHE {
                self.cache.pop_front();
            }
            self.cache.push_back((key, prep));
        }
        let (_, prep) = self.cache.back().expect("just pushed or promoted");
        Ok(prep)
    }

    /// Drop cached frames whose grid no longer matches the current
    /// terminal / config. Called on toggles that change the rendered
    /// grid.
    fn invalidate_cache(&mut self) {
        self.cache.clear();
    }
}

impl Mode for SvgAnimationMode {
    fn id(&self) -> ModeId {
        ModeId::Animation
    }

    fn label(&self) -> &str {
        "Animation"
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, _rows: usize) -> Result<Window> {
        let term = self.view.prepare_term(ctx);
        self.last_term = Some(term);
        self.prepare_current(term)?;
        let (_, prep) = self.cache.back().expect("prepare_current pushed");
        Ok(self.view.render_prepared(prep, term))
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_to_pipe(
        &mut self,
        ctx: &RenderCtx,
        out: &mut crate::output::PrintOutput,
    ) -> Result<()> {
        let snap = self.view.pipe_snapshot();
        self.invalidate_cache();
        let capped = ctx.capped_for_image_pipe();
        let window = self.render_window(&capped, 0, capped.term_rows)?;
        ImageView::write_lines(out, window)?;
        self.view.restore(snap);
        Ok(())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        // If we've rendered at least once, prep dims for the current
        // frame are stable per (frame_idx, term, fit) — clamp like
        // ImageRenderMode does. Before first render, fall back to the
        // optimistic path; render_window will clamp on next draw.
        let bounds = match (self.last_term, self.cache.back()) {
            (Some(term), Some((_, prep))) => {
                let b = self.view.view_bounds(prep, term);
                ScrollBounds::clamped(b.max_x, b.max_y, b.viewport_rows.saturating_sub(1))
            }
            _ => ScrollBounds::unbounded(),
        };
        self.view.scroll(action, bounds)
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        SVG_ANIM_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        if let Some(h) = self.view.handle_config_cycle(action) {
            // Any image-config change invalidates the rasterized-frame
            // cache — the new grid won't match the cached entries.
            self.invalidate_cache();
            return h;
        }
        if let (Some(term), Some((_, prep))) = (self.last_term, self.cache.back()) {
            let bounds = self.view.view_bounds(prep, term);
            if let Some(h) = self.view.handle_zoom(action, bounds) {
                return h;
            }
        }
        match action {
            Action::PlayPause => self.anim.play_pause(),
            Action::Next => self.anim.step(self.model.frames.len(), true),
            Action::Prev => self.anim.step(self.model.frames.len(), false),
            _ => Handled::No,
        }
    }

    fn next_tick(&self) -> Option<Duration> {
        self.anim
            .next_tick(self.model.frames[self.anim.current].delay)
    }

    fn tick(&mut self) -> bool {
        self.anim.tick(self.model.frames.len())
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let mut segs = self.view.status_segments(theme);
        segs.push((
            self.anim.status_segment(self.model.frames.len()),
            theme.label,
        ));
        segs
    }

    fn extract_target(&self) -> Option<ExtractTarget> {
        Some(self.anim.extract_target())
    }
}
