//! [`LogoAnimation`] — the animated variant of the About-screen logo.
//!
//! Two time-driven effects layered over the same [`crate::output::LOGO`]
//! glyphs the static painter uses:
//!
//! - **Rotating gradient.** The value→heading gradient slides across the
//!   wordmark, mirrored at the wrap point so the colors ping-pong without
//!   a visible seam.
//! - **Edge flash.** Periodically, two short bright runners leave the
//!   logo's top-left-most glyph, trace the wordmark's outline in opposite
//!   directions (one clockwise, one counter-clockwise), and meet half a
//!   loop later where they shrink away. Each runner is up to three cells
//!   long with the middle cell brightest. White on dark themes, black on
//!   light ones.
//!
//! Painting goes through [`crate::output::paint_logo_with`] — the same
//! glyph walk as the static `paint_logo` the print path (`--help`,
//! version screen) uses; only the per-glyph color closure differs.
//! Driven by the standard `Mode::next_tick` / `tick` contract — no
//! timers of its own. About skips the animation entirely in plain mode.

use std::time::Duration;

use syntect::highlighting::Color;

use crate::output::{LOGO, paint_logo_with};
use peek_theme::{PeekTheme, lerp_color, rgb_to_luminance};

/// Time between animation ticks (~20 fps, plenty for a 21-column glyph).
const TICK: Duration = Duration::from_millis(50);
/// Gradient phase advance per tick. Deliberately not a round divisor of
/// the flash cadence, so the gradient sits at a different phase on each
/// flash and the combined animation drifts instead of looping exactly.
const PHASE_STEP: f32 = 0.00618;
/// Ticks of idle gradient rotation between edge flashes.
const FLASH_IDLE_TICKS: u32 = 120;
/// Runner trail length in cells (head + this many behind it).
const TRAIL: usize = 2;
/// Highlight strength of the trail cells beside the brightest one.
const TRAIL_EDGE: f32 = 0.55;

pub struct LogoAnimation {
    /// Gradient phase in `[0, 1)`; `0.0` reproduces the static logo.
    phase: f32,
    /// Ticks remaining until the next flash starts. Counts down only
    /// while no flash is running.
    idle_ticks: u32,
    /// Step of the running flash (`None` = idle). Each tick advances the
    /// runners one cell along their half of the outline loop.
    flash_step: Option<usize>,
    /// Closed outline loop around the wordmark, as `(row, col)` cells.
    /// `loop_path[0]` is the top-left-most glyph; the runners traverse
    /// index `+s` and `-s` from it and meet half a loop away.
    loop_path: Vec<(usize, usize)>,
}

impl LogoAnimation {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            idle_ticks: FLASH_IDLE_TICKS / 2,
            flash_step: None,
            loop_path: outline_loop(LOGO),
        }
    }

    /// Always ticking while visible: the gradient rotates constantly.
    pub fn next_tick(&self) -> Option<Duration> {
        Some(TICK)
    }

    /// Advance one frame. Always returns `true` — the gradient moves
    /// every tick, so every tick is a redraw.
    pub fn tick(&mut self) -> bool {
        self.phase = (self.phase + PHASE_STEP).fract();
        match self.flash_step {
            None => {
                if self.idle_ticks == 0 {
                    self.flash_step = Some(0);
                } else {
                    self.idle_ticks -= 1;
                }
            }
            Some(step) => {
                // Done once the tail has shrunk into the meeting cell.
                if step > self.half_len() + TRAIL {
                    self.flash_step = None;
                    self.idle_ticks = FLASH_IDLE_TICKS;
                } else {
                    self.flash_step = Some(step + 1);
                }
            }
        }
        true
    }

    /// Paint the logo with the current gradient phase and flash overlay.
    /// Same output shape as [`crate::output::paint_logo`]: one styled
    /// String per logo line.
    pub fn paint(&self, pt: &PeekTheme) -> Vec<String> {
        let flash = self.flash_cells();
        let flash_color = flash_color(pt);
        paint_logo_with(pt, |row, col, t| {
            let mut color = lerp_color(pt.value, pt.heading, sliding(t, self.phase));
            if let Some((_, strength)) = flash.iter().find(|(cell, _)| *cell == (row, col)) {
                color = lerp_color(color, flash_color, *strength);
            }
            color
        })
    }

    /// Loop distance each runner covers — they meet at the far side.
    fn half_len(&self) -> usize {
        self.loop_path.len() / 2
    }

    /// Cells lit by the current flash step, with highlight strength.
    /// Two runners: clockwise (`+i`) and counter-clockwise (`-i`) from
    /// `loop_path[0]`, each trailing up to [`TRAIL`] cells whose middle
    /// is brightest.
    fn flash_cells(&self) -> Vec<((usize, usize), f32)> {
        let Some(step) = self.flash_step else {
            return Vec::new();
        };
        let len = self.loop_path.len();
        if len == 0 {
            return Vec::new();
        }
        let half = self.half_len();
        let mut cells = Vec::new();
        // Trail window [step - TRAIL, step], clamped to the half-loop;
        // the lower clamp makes the runner grow out of the start corner,
        // the upper one shrinks it into the meeting cell.
        let lo = step.saturating_sub(TRAIL);
        for i in lo..=step.min(half) {
            // Middle of a full 3-cell trail is `step - 1`.
            let strength = if i + 1 == step { 1.0 } else { TRAIL_EDGE };
            cells.push((self.loop_path[i % len], strength));
            cells.push((self.loop_path[(len - i % len) % len], strength));
        }
        cells
    }
}

impl Default for LogoAnimation {
    fn default() -> Self {
        Self::new()
    }
}

/// Slide the gradient: at `phase` 0 this is the identity (the static
/// gradient), and advancing the phase shifts the ramp along the logo,
/// mirrored at the wrap so the sweep reverses instead of jumping.
fn sliding(t: f32, phase: f32) -> f32 {
    let x = (t * 0.5 + phase).fract();
    1.0 - (2.0 * x - 1.0).abs()
}

/// Flash highlight color: white on dark backgrounds, black on light.
fn flash_color(pt: &PeekTheme) -> Color {
    let bg = pt.background;
    let v = if rgb_to_luminance(bg.r, bg.g, bg.b) > 128 {
        0
    } else {
        255
    };
    Color {
        r: v,
        g: v,
        b: v,
        a: 255,
    }
}

/// Closed loop of `(row, col)` cells tracing the wordmark's outline:
/// per-column topmost glyphs left→right, then per-column bottommost
/// glyphs right→left (columns whose top and bottom coincide contribute
/// one cell per side). Rotated so index 0 is the top-left-most glyph
/// (minimal `row + col`), which is where the flash runners start.
fn outline_loop(logo: &[&str]) -> Vec<(usize, usize)> {
    let width = logo.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut tops = Vec::new();
    let mut bottoms = Vec::new();
    for col in 0..width {
        let mut top = None;
        let mut bottom = None;
        for (row, line) in logo.iter().enumerate() {
            if line.chars().nth(col).is_some_and(|c| c != ' ') {
                top.get_or_insert(row);
                bottom = Some(row);
            }
        }
        if let (Some(t), Some(b)) = (top, bottom) {
            tops.push((t, col));
            bottoms.push((b, col));
        }
    }
    let mut path = tops;
    path.extend(bottoms.into_iter().rev());
    path.dedup();
    if path.len() > 1 && path.first() == path.last() {
        path.pop();
    }
    // Start the runners at the visually top-left glyph.
    if let Some(start) = (0..path.len()).min_by_key(|&i| {
        let (r, c) = path[i];
        (r + c, c)
    }) {
        path.rotate_left(start);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliding_phase_zero_is_identity() {
        for t in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            assert!((sliding(t, 0.0) - t).abs() < 1e-5);
        }
    }

    #[test]
    fn sliding_stays_in_unit_range() {
        for step in 0..200 {
            let phase = step as f32 * 0.013;
            for t in [0.0_f32, 0.33, 0.66, 1.0] {
                let v = sliding(t, phase.fract());
                assert!((0.0..=1.0).contains(&v), "sliding({t}, {phase}) = {v}");
            }
        }
    }

    #[test]
    fn outline_loop_covers_logo_extremes() {
        let path = outline_loop(LOGO);
        assert!(!path.is_empty());
        // Every cell is a real glyph.
        for &(row, col) in &path {
            let ch = LOGO[row].chars().nth(col).unwrap();
            assert_ne!(ch, ' ', "({row},{col}) is blank");
        }
        // Both profiles present: some cell from the top row region and
        // the bottom row.
        assert!(path.iter().any(|&(r, _)| r == 0));
        assert!(path.iter().any(|&(r, _)| r + 1 == LOGO.len()));
    }

    #[test]
    fn flash_runs_to_completion_and_goes_idle() {
        let mut anim = LogoAnimation::new();
        anim.idle_ticks = 0;
        anim.tick(); // starts the flash
        assert!(anim.flash_step.is_some());
        let limit = anim.loop_path.len() + TRAIL + 4;
        for _ in 0..limit {
            anim.tick();
            if anim.flash_step.is_none() {
                break;
            }
        }
        assert!(anim.flash_step.is_none());
        // The completing tick re-arms the idle countdown in full.
        assert_eq!(anim.idle_ticks, FLASH_IDLE_TICKS);
    }

    #[test]
    fn flash_cells_lie_on_the_loop() {
        let mut anim = LogoAnimation::new();
        anim.idle_ticks = 0;
        for _ in 0..(anim.half_len() + TRAIL + 2) {
            anim.tick();
            for (cell, strength) in anim.flash_cells() {
                assert!(anim.loop_path.contains(&cell));
                assert!((0.0..=1.0).contains(&strength));
            }
        }
    }
}

/// Visual tuning aid, not a regression test. Dumps every 4th flash frame
/// as ASCII (`#` = bright cell, `+` = trail edge, `.` = unlit glyph) so
/// the runner paths can be eyeballed after changing the loop or trail:
/// `cargo test -p peek-foundation dump_flash_frames -- --ignored --nocapture`
#[cfg(test)]
mod viz {
    use super::*;

    #[test]
    #[ignore]
    fn dump_flash_frames() {
        let mut anim = LogoAnimation::new();
        anim.idle_ticks = 0;
        for frame in 0..=(anim.half_len() + TRAIL + 2) {
            anim.tick();
            if frame % 4 != 0 {
                continue;
            }
            println!("--- step {:?}", anim.flash_step);
            let cells = anim.flash_cells();
            for (row, line) in LOGO.iter().enumerate() {
                let rendered: String = line
                    .chars()
                    .enumerate()
                    .map(
                        |(col, ch)| match cells.iter().find(|(c, _)| *c == (row, col)) {
                            Some((_, s)) if *s >= 1.0 => '#',
                            Some(_) => '+',
                            None if ch == ' ' => ' ',
                            None => '.',
                        },
                    )
                    .collect();
                println!("{rendered}");
            }
        }
    }
}
