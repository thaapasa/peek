//! Animated SVG view: CSS `@keyframes` driven playback. Mirrors
//! [`AnimationMode`] (GIF/WebP) for keybindings, scroll, and status, but
//! rasterizes each frame on demand from a parsed [`AnimatedSvg`] model
//! rather than holding pre-decoded pixel buffers. A bounded LRU caches
//! recently composited frames keyed by `(frame_idx, grid_cols, grid_rows)`
//! so playback after one full loop becomes free, and switching fit/mode
//! invalidates only the entries whose grid no longer matches.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use syntect::highlighting::Color;

use super::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::theme::PeekTheme;
use crate::viewer::image::render::{self, GridWindow, PreparedImage, TermSize};
use crate::viewer::image::svg_anim::{self, AnimatedSvg};
use crate::viewer::image::{FitMode, ImageConfig};
use crate::viewer::ui::Action;

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
}

pub(crate) struct SvgAnimationMode {
    model: Arc<AnimatedSvg>,
    current: usize,
    playing: bool,
    config: ImageConfig,
    last_advance: Instant,
    scroll_x: u32,
    scroll_y: u32,
    /// Bounded LRU of prepared frames. We push fresh entries to the back
    /// and evict from the front when full.
    cache: VecDeque<(CacheKey, PreparedImage)>,
}

const SVG_ANIM_ACTIONS: &[(Action, &str)] = &[
    (Action::PlayPause, "Play / pause"),
    (Action::NextFrame, "Next frame"),
    (Action::PrevFrame, "Previous frame"),
    (Action::CycleBackground, "Cycle background (images)"),
    (Action::CycleImageMode, "Cycle render mode (images)"),
    (Action::CycleFitMode, "Cycle fit (contain / width / height)"),
    (Action::ScrollLeft, "Scroll left (FitHeight)"),
    (Action::ScrollRight, "Scroll right (FitHeight)"),
];

impl SvgAnimationMode {
    pub(crate) fn new(model: AnimatedSvg, config: ImageConfig) -> Self {
        assert!(
            !model.frames.is_empty(),
            "SvgAnimationMode requires \u{2265}1 frame"
        );
        Self {
            model: Arc::new(model),
            current: 0,
            playing: true,
            config,
            last_advance: Instant::now(),
            scroll_x: 0,
            scroll_y: 0,
            cache: VecDeque::with_capacity(FRAME_CACHE),
        }
    }

    fn prepare_current(&mut self, term: TermSize) -> Result<&PreparedImage> {
        // Probe with a dry-run prepare to learn (cols, rows) for the cache
        // key — they depend on term + fit + margin + svg dims. Cheap: it's
        // a few arithmetic ops, no rasterization.
        let (probe_cols, probe_rows) = render::compute_grid(
            self.model.width_px + self.config.margin * 2,
            self.model.height_px + self.config.margin * 2,
            term,
            self.config.width,
            self.config.fit,
        );
        let key = CacheKey {
            frame_idx: self.current as u32,
            cols: probe_cols,
            rows: probe_rows,
            margin: self.config.margin,
            ascii: matches!(self.config.mode, crate::viewer::image::ImageMode::Ascii),
            fit: self.config.fit,
        };

        if let Some(pos) = self.cache.iter().position(|(k, _)| *k == key) {
            // Move to back (mark MRU) by removing + re-inserting.
            let entry = self.cache.remove(pos).unwrap();
            self.cache.push_back(entry);
            return Ok(&self.cache.back().unwrap().1);
        }

        let svg_text = svg_anim::render_frame(&self.model, self.current);
        let prep = render::prepare_svg_bytes(
            svg_text.as_bytes(),
            self.model.width_px,
            self.model.height_px,
            &self.config,
            term,
        )?;

        if self.cache.len() == FRAME_CACHE {
            self.cache.pop_front();
        }
        self.cache.push_back((key, prep));
        Ok(&self.cache.back().unwrap().1)
    }

    /// Drop cached frames whose grid no longer matches the current
    /// terminal/config. Called on toggles that change the rendered grid.
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
        self.config.color_mode = ctx.peek_theme.color_mode;
        let term = TermSize {
            cols: ctx.term_cols.min(u32::MAX as usize) as u32,
            rows: ctx.term_rows.min(u32::MAX as usize) as u32,
        };
        self.prepare_current(term)?;
        let prep = &self.cache.back().unwrap().1;
        let (cols, rows) = (prep.cols, prep.rows);

        let (max_x, max_y) = render::max_scroll(cols, rows, term.cols, term.rows);
        self.scroll_x = self.scroll_x.min(max_x);
        self.scroll_y = self.scroll_y.min(max_y);
        let visible_cols = cols.min(term.cols);
        let visible_rows = rows.min(term.rows);
        let window = GridWindow {
            col_start: self.scroll_x,
            col_end: self.scroll_x + visible_cols,
            row_start: self.scroll_y,
            row_end: self.scroll_y + visible_rows,
        };

        let prep = &self.cache.back().unwrap().1;
        let lines = render::render_prepared(prep, &self.config, window);
        let total = rows as usize;
        Ok(Window { lines, total })
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_to_pipe(
        &mut self,
        ctx: &RenderCtx,
        out: &mut crate::output::PrintOutput,
    ) -> Result<()> {
        let saved_fit = self.config.fit;
        let (saved_x, saved_y) = (self.scroll_x, self.scroll_y);
        self.config.fit = FitMode::Contain;
        self.scroll_x = 0;
        self.scroll_y = 0;
        self.invalidate_cache();
        let window = self.render_window(ctx, 0, ctx.term_rows)?;
        for line in window.lines {
            out.write_line(&line)?;
        }
        self.config.fit = saved_fit;
        self.scroll_x = saved_x;
        self.scroll_y = saved_y;
        Ok(())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        const HSTEP: u32 = 4;
        const VPAGE: u32 = 20;
        match action {
            Action::ScrollUp => {
                self.scroll_y = self.scroll_y.saturating_sub(1);
                true
            }
            Action::ScrollDown => {
                self.scroll_y = self.scroll_y.saturating_add(1);
                true
            }
            Action::PageUp => {
                self.scroll_y = self.scroll_y.saturating_sub(VPAGE);
                true
            }
            Action::PageDown => {
                self.scroll_y = self.scroll_y.saturating_add(VPAGE);
                true
            }
            Action::Top => {
                self.scroll_x = 0;
                self.scroll_y = 0;
                true
            }
            Action::Bottom => {
                self.scroll_y = u32::MAX;
                true
            }
            Action::ScrollLeft => {
                self.scroll_x = self.scroll_x.saturating_sub(HSTEP);
                true
            }
            Action::ScrollRight => {
                self.scroll_x = self.scroll_x.saturating_add(HSTEP);
                true
            }
            _ => false,
        }
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        SVG_ANIM_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::PlayPause => {
                self.playing = !self.playing;
                if self.playing {
                    self.last_advance = Instant::now();
                }
                Handled::Yes
            }
            Action::NextFrame => {
                self.current = (self.current + 1) % self.model.frames.len();
                self.last_advance = Instant::now();
                Handled::Yes
            }
            Action::PrevFrame => {
                let n = self.model.frames.len();
                self.current = (self.current + n - 1) % n;
                self.last_advance = Instant::now();
                Handled::Yes
            }
            Action::CycleBackground => {
                self.config.background = self.config.background.next();
                self.invalidate_cache();
                Handled::Yes
            }
            Action::CycleImageMode => {
                self.config.mode = self.config.mode.next();
                self.invalidate_cache();
                Handled::Yes
            }
            Action::CycleFitMode => {
                self.config.fit = self.config.fit.next();
                self.scroll_x = 0;
                self.scroll_y = 0;
                self.invalidate_cache();
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn next_tick(&self) -> Option<Duration> {
        if !self.playing {
            return None;
        }
        let elapsed = self.last_advance.elapsed();
        Some(
            self.model.frames[self.current]
                .delay
                .saturating_sub(elapsed),
        )
    }

    fn tick(&mut self) -> bool {
        self.current = (self.current + 1) % self.model.frames.len();
        self.last_advance = Instant::now();
        true
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let play_icon = if self.playing { "\u{25b6}" } else { "\u{23f8}" };
        let frame_info = format!(
            "Frame {}/{} {}",
            self.current + 1,
            self.model.frames.len(),
            play_icon
        );
        vec![
            (self.config.mode.label().to_string(), theme.label),
            (self.config.fit.label().to_string(), theme.label),
            (frame_info, theme.label),
        ]
    }
}
