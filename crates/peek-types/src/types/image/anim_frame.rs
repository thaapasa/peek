//! [`AnimFrameState`] — shared frame-playback state for animated image
//! Modes ([`super::animation_mode::AnimationMode`] for GIF / WebP and
//! [`crate::types::svg::animation_mode::SvgAnimationMode`] for CSS
//! `@keyframes`). Owns `current` frame index, play/pause flag, and the
//! `last_advance` clock that drives `next_tick`. Each Mode owns its
//! own frame list (decoded pixels vs SVG keyframe model); this struct
//! is the position/clock state both share.

use std::time::{Duration, Instant};

use crate::viewer::modes::{ExtractTarget, Handled};

/// Frame-position + play state shared by every animated image Mode.
pub(crate) struct AnimFrameState {
    pub current: usize,
    pub playing: bool,
    last_advance: Instant,
}

impl AnimFrameState {
    pub fn new() -> Self {
        Self {
            current: 0,
            playing: true,
            last_advance: Instant::now(),
        }
    }

    pub fn play_pause(&mut self) -> Handled {
        self.playing = !self.playing;
        if self.playing {
            self.last_advance = Instant::now();
        }
        Handled::Yes
    }

    /// Step one frame forward (`forward = true`) or backward, wrapping
    /// at the boundary. No-ops on an empty frame list.
    pub fn step(&mut self, len: usize, forward: bool) -> Handled {
        if len == 0 {
            return Handled::No;
        }
        self.current = if forward {
            (self.current + 1) % len
        } else {
            (self.current + len - 1) % len
        };
        self.last_advance = Instant::now();
        Handled::Yes
    }

    /// Auto-advance one frame (forward, wrapping). Returns `false` on
    /// an empty list — caller can short-circuit the redraw.
    pub fn tick(&mut self, len: usize) -> bool {
        if len == 0 {
            return false;
        }
        self.current = (self.current + 1) % len;
        self.last_advance = Instant::now();
        true
    }

    /// Remaining time until the next auto-advance. `None` when paused.
    /// `frame_delay` is the current frame's per-frame delay from the
    /// source (GIF graphic-control extension / WebP frame duration /
    /// SVG keyframe interval).
    pub fn next_tick(&self, frame_delay: Duration) -> Option<Duration> {
        if !self.playing {
            return None;
        }
        let elapsed = self.last_advance.elapsed();
        Some(frame_delay.saturating_sub(elapsed))
    }

    /// `"Frame N/M ▶"` (or `"⏸"` when paused). 1-based to match what
    /// the user sees.
    pub fn status_segment(&self, len: usize) -> String {
        let play_icon = if self.playing { "\u{25b6}" } else { "\u{23f8}" };
        format!("Frame {}/{} {}", self.current + 1, len, play_icon)
    }

    /// 1-based frame index for the `Extract current frame as PNG`
    /// action — matches the visible "Frame N/M" counter.
    pub fn extract_target(&self) -> ExtractTarget {
        ExtractTarget::FrameIndex(self.current + 1)
    }
}
