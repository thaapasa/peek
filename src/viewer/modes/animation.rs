use std::time::{Duration, Instant};

use anyhow::Result;
use syntect::highlighting::Color;

use super::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::theme::PeekTheme;
use crate::viewer::image::animate::AnimFrame;
use crate::viewer::image::render::{self, GridWindow, TermSize};
use crate::viewer::image::{FitMode, ImageConfig};
use crate::viewer::ui::Action;

/// Animated image view (GIF/WebP). Owns the decoded frame list, current
/// frame index, play/pause state, and image config (background + fit
/// mode + scroll). Drives frame advancement via the `next_tick` / `tick`
/// hooks on `Mode`.
///
/// Fit handling mirrors `ImageRenderMode`: under `FitWidth` / `FitHeight`
/// the prepared frame grid may exceed the terminal viewport on one axis;
/// `scroll_x` / `scroll_y` track the offset. Scroll persists across
/// frame ticks (panning a long banner GIF stays put while frames cycle).
/// Toggling fit resets scroll. Each frame is independently
/// prepare→composite→rendered; we don't cache between frames because the
/// underlying `DynamicImage` changes every tick.
pub(crate) struct AnimationMode {
    frames: Vec<AnimFrame>,
    current: usize,
    playing: bool,
    config: ImageConfig,
    last_advance: Instant,
    scroll_x: u32,
    scroll_y: u32,
}

const ANIM_ACTIONS: &[(Action, &str)] = &[
    (Action::PlayPause, "Play / pause"),
    (Action::NextFrame, "Next frame"),
    (Action::PrevFrame, "Previous frame"),
    (Action::CycleBackground, "Cycle background (images)"),
    (Action::CycleImageMode, "Cycle render mode (images)"),
    (Action::CycleFitMode, "Cycle fit (contain / width / height)"),
    (Action::ScrollLeft, "Scroll left (FitHeight)"),
    (Action::ScrollRight, "Scroll right (FitHeight)"),
];

impl AnimationMode {
    pub(crate) fn new(frames: Vec<AnimFrame>, config: ImageConfig) -> Self {
        assert!(!frames.is_empty(), "AnimationMode requires \u{2265}1 frame");
        Self {
            frames,
            current: 0,
            playing: true,
            config,
            last_advance: Instant::now(),
            scroll_x: 0,
            scroll_y: 0,
        }
    }

    fn max_scroll(prep_cols: u32, prep_rows: u32, term_cols: u32, term_rows: u32) -> (u32, u32) {
        (
            prep_cols.saturating_sub(term_cols),
            prep_rows.saturating_sub(term_rows),
        )
    }
}

impl Mode for AnimationMode {
    fn id(&self) -> ModeId {
        ModeId::Animation
    }

    fn label(&self) -> &str {
        "Animation"
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, _rows: usize) -> Result<Window> {
        // ColorMode can change between renders (interactive cycle).
        self.config.color_mode = ctx.peek_theme.color_mode;
        let term = TermSize {
            cols: ctx.term_cols.min(u32::MAX as usize) as u32,
            rows: ctx.term_rows.min(u32::MAX as usize) as u32,
        };
        let frame = &self.frames[self.current];
        let prep = render::prepare_decoded(frame.image.clone(), &self.config, term);

        let (max_x, max_y) = Self::max_scroll(prep.cols, prep.rows, term.cols, term.rows);
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

        let lines = render::render_prepared(&prep, &self.config, window);
        let total = prep.rows as usize;
        Ok(Window { lines, total })
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    /// Pipe / `--print` always renders the current frame at `Contain`. The
    /// pipe path renders one frame (no animation in stdout), and unbounded
    /// rows make `FitHeight` meaningless / `FitWidth` redundant.
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
        // We don't keep the prepared grid bounds between calls (frames
        // change on every tick), so this handler just nudges the offsets
        // optimistically. The real clamp lives in `render_window`, which
        // computes max_scroll against the live frame and pulls the
        // saturated-add value back to the actual bound.
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
        ANIM_ACTIONS
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
                self.current = (self.current + 1) % self.frames.len();
                self.last_advance = Instant::now();
                Handled::Yes
            }
            Action::PrevFrame => {
                let n = self.frames.len();
                self.current = (self.current + n - 1) % n;
                self.last_advance = Instant::now();
                Handled::Yes
            }
            Action::CycleBackground => {
                self.config.background = self.config.background.next();
                Handled::Yes
            }
            Action::CycleImageMode => {
                self.config.mode = self.config.mode.next();
                Handled::Yes
            }
            Action::CycleFitMode => {
                self.config.fit = self.config.fit.next();
                self.scroll_x = 0;
                self.scroll_y = 0;
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
        Some(self.frames[self.current].delay.saturating_sub(elapsed))
    }

    fn tick(&mut self) -> bool {
        self.current = (self.current + 1) % self.frames.len();
        self.last_advance = Instant::now();
        true
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let play_icon = if self.playing { "\u{25b6}" } else { "\u{23f8}" };
        let frame_info = format!(
            "Frame {}/{} {}",
            self.current + 1,
            self.frames.len(),
            play_icon
        );
        vec![
            (self.config.mode.label().to_string(), theme.label),
            (self.config.fit.label().to_string(), theme.label),
            (frame_info, theme.label),
        ]
    }
}
