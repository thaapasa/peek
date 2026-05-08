//! Detect the terminal's actual cell aspect ratio so image rendering
//! preserves source aspect across fonts.
//!
//! Pulls cell pixel dimensions from `TIOCGWINSZ` (`crossterm::terminal::
//! window_size`). Most modern terminals (iTerm2, kitty, WezTerm, Ghostty,
//! Alacritty, xterm) fill `ws_xpixel` / `ws_ypixel`; basic xterm and
//! some tmux versions report 0 there, in which case we fall back to the
//! conventional 1:2 cell aspect (cell twice as tall as wide).
//!
//! Result is cached on first call: terminals don't change cell sizes
//! mid-run, and re-querying on every render would be wasted ioctl calls.

use std::sync::OnceLock;

/// Conventional cell aspect (height / width) when detection fails or
/// the terminal can't report cell pixel size. Two-cell-tall is what
/// peek's glyph atlas itself is built for, so the fallback also keeps
/// rendering consistent with no-detect environments.
const DEFAULT_ASPECT: f64 = 2.0;

/// Sanity bounds on detected aspect — anything outside this range is
/// treated as a malformed report and we fall back to the default.
/// 1.0 (square cells) and 4.0 (4×-tall cells) bracket every realistic
/// terminal font.
const ASPECT_MIN: f64 = 1.0;
const ASPECT_MAX: f64 = 4.0;

/// Cell aspect (height / width) for the current terminal. Cached on
/// first read; an explicit user override stored via [`set_override`]
/// wins over auto-detection and the default fallback.
pub fn cell_aspect_h_over_w() -> f64 {
    *CACHE.get_or_init(detect_or_default)
}

/// User-supplied override (CLI `--cell-aspect`). Must be called before
/// the first `cell_aspect_h_over_w()` consumer. Out-of-range values
/// are ignored so a typo can't render images at 0×∞.
pub fn set_override(aspect: f64) {
    if !(ASPECT_MIN..=ASPECT_MAX).contains(&aspect) {
        return;
    }
    let _ = CACHE.set(aspect);
}

static CACHE: OnceLock<f64> = OnceLock::new();

fn detect_or_default() -> f64 {
    let Ok(ws) = crossterm::terminal::window_size() else {
        return DEFAULT_ASPECT;
    };
    if ws.width == 0 || ws.height == 0 || ws.columns == 0 || ws.rows == 0 {
        return DEFAULT_ASPECT;
    }
    let cell_w = ws.width as f64 / ws.columns as f64;
    let cell_h = ws.height as f64 / ws.rows as f64;
    if cell_w <= 0.0 || cell_h <= 0.0 {
        return DEFAULT_ASPECT;
    }
    let aspect = cell_h / cell_w;
    if (ASPECT_MIN..=ASPECT_MAX).contains(&aspect) {
        aspect
    } else {
        DEFAULT_ASPECT
    }
}
