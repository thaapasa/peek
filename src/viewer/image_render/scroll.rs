//! Shared scroll-action handler for image-grid view modes
//! (`ImageRenderMode`, `AnimationMode`, `SvgAnimationMode`).
//!
//! All three modes own their own `(scroll_x, scroll_y)` because the
//! global line scroller doesn't apply to a 2D grid. They also share the
//! same key-action set: arrows, PgUp/PgDn, Home/End. The only thing that
//! varies is whether the mode knows its bounds at scroll time:
//!
//! - `ImageRenderMode` and `SvgAnimationMode` cache the prepared frame
//!   plus the terminal it was prepared for, so they can clamp
//!   authoritatively.
//! - `AnimationMode` re-decodes each tick, so live bounds aren't known
//!   here — `Bounds::unbounded()` lets the offset run free, and the
//!   render path clamps against the freshly prepared frame.
//!
//! Keeping the dispatch in one helper means the handful of subtle cases
//! (Bottom = max_y vs `u32::MAX`, page step = `term_rows-1` vs a hard-
//! coded constant) live in one place rather than drifting across three
//! near-identical match arms.

use crate::viewer::ui::Action;

/// Arrow-key horizontal step. Small enough to feel responsive, big
/// enough to cross a wide image. Page-equivalent stays vertical.
pub(crate) const HSTEP: u32 = 4;

/// Fallback vertical page size when terminal height isn't known at
/// scroll time (`AnimationMode`'s decode-per-tick path).
pub(crate) const FALLBACK_PAGE_Y: u32 = 20;

#[derive(Copy, Clone)]
pub(crate) struct ScrollBounds {
    pub max_x: u32,
    pub max_y: u32,
    pub page_y: u32,
    pub hstep: u32,
}

impl ScrollBounds {
    pub fn clamped(max_x: u32, max_y: u32, page_y: u32) -> Self {
        Self {
            max_x,
            max_y,
            page_y: page_y.max(1),
            hstep: HSTEP,
        }
    }

    /// Bounds when live grid dims aren't available — saturating ops only;
    /// the render path is responsible for the final clamp.
    pub fn unbounded() -> Self {
        Self {
            max_x: u32::MAX,
            max_y: u32::MAX,
            page_y: FALLBACK_PAGE_Y,
            hstep: HSTEP,
        }
    }
}

/// Apply a scroll action to `(scroll_x, scroll_y)`. Returns true when
/// the action was a scroll the mode handled.
pub(crate) fn apply(
    scroll_x: &mut u32,
    scroll_y: &mut u32,
    action: Action,
    b: ScrollBounds,
) -> bool {
    match action {
        Action::ScrollUp => {
            *scroll_y = scroll_y.saturating_sub(1);
            true
        }
        Action::ScrollDown => {
            *scroll_y = scroll_y.saturating_add(1).min(b.max_y);
            true
        }
        Action::PageUp => {
            *scroll_y = scroll_y.saturating_sub(b.page_y);
            true
        }
        Action::PageDown => {
            *scroll_y = scroll_y.saturating_add(b.page_y).min(b.max_y);
            true
        }
        Action::Top => {
            *scroll_x = 0;
            *scroll_y = 0;
            true
        }
        Action::Bottom => {
            *scroll_y = b.max_y;
            true
        }
        Action::ScrollLeft => {
            *scroll_x = scroll_x.saturating_sub(b.hstep);
            true
        }
        Action::ScrollRight => {
            *scroll_x = scroll_x.saturating_add(b.hstep).min(b.max_x);
            true
        }
        _ => false,
    }
}
