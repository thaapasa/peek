use std::time::{Duration, Instant};

use anyhow::Result;
use syntect::highlighting::Color;

use super::{Handled, Mode, ModeId, RenderCtx};
use crate::theme::PeekTheme;
use crate::viewer::image::ImageConfig;
use crate::viewer::image::animate::{AnimFrame, render_frame};
use crate::viewer::ui::Action;

/// Animated image view (GIF/WebP). Owns the decoded frame list, current
/// frame index, play/pause state, and image config (for background cycle).
/// Drives frame advancement via the `next_tick` / `tick` hooks on `Mode`.
pub(crate) struct AnimationMode {
    frames: Vec<AnimFrame>,
    current: usize,
    playing: bool,
    config: ImageConfig,
    last_advance: Instant,
}

const ANIM_ACTIONS: &[(Action, &str)] = &[
    (Action::PlayPause, "Play / pause"),
    (Action::NextFrame, "Next frame"),
    (Action::PrevFrame, "Previous frame"),
    (Action::CycleBackground, "Cycle background (images)"),
    (Action::CycleImageMode, "Cycle render mode (images)"),
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
        }
    }
}

impl Mode for AnimationMode {
    fn id(&self) -> ModeId {
        ModeId::Animation
    }

    fn label(&self) -> &str {
        "Animation"
    }

    fn render(&mut self, _ctx: &RenderCtx) -> Result<Vec<String>> {
        Ok(render_frame(&self.frames[self.current], &self.config))
    }

    fn rerender_on_resize(&self) -> bool {
        true
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
            (frame_info, theme.label),
        ]
    }
}
