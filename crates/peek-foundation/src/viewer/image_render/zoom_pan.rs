//! [`ZoomPanState`] — shared zoom + pan apparatus for every Mode that
//! navigates a zoomable image grid. Embedded by [`super::view::ImageView`]
//! (static / animated images, SVG keyframes, font specimen) and by
//! [`crate::viewer::paged::PagedImageMode`] (PDF, CBZ).
//!
//! Bundles `(zoom, scroll_x, scroll_y)` with the two interactive
//! handlers that act on them: [`ZoomPanState::handle_zoom`] (anchored
//! zoom around the viewport centre) and [`ZoomPanState::scroll`]
//! (delegates to the shared 2D scroll handler). The fields are
//! `pub` so callers that need direct mutation (post-render
//! clamping, fit-mode reset) can write them without going through an
//! accessor zoo.

use super::scroll::{self, ScrollBounds};
use super::zoom::{ZoomLevel, anchor_zoom_change};
use crate::viewer::modes::Handled;
use crate::viewer::ui::Action;

/// Scroll bounds suitable for the active view: the effective grid
/// minus the terminal-clamped viewport, on each axis. Used by every
/// caller that needs to clamp pan after a scroll action so zoom > 1
/// pan ranges respect the larger effective grid.
#[derive(Copy, Clone, Debug)]
pub struct ViewBounds {
    pub max_x: u32,
    pub max_y: u32,
    pub viewport_cols: u32,
    pub viewport_rows: u32,
}

/// Zoom level + 2D pan offset. Embedded by every Mode that pans a
/// zoomable grid (image / SVG / paged document / font specimen).
#[derive(Copy, Clone, Debug, Default)]
pub struct ZoomPanState {
    pub zoom: ZoomLevel,
    pub scroll_x: u32,
    pub scroll_y: u32,
}

impl ZoomPanState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset pan to origin without touching zoom.
    pub fn reset_pan(&mut self) {
        self.scroll_x = 0;
        self.scroll_y = 0;
    }

    /// Pass a scroll action to the shared 2D scroll handler with the
    /// caller-supplied bounds (clamped to live prep dims, or unbounded
    /// for per-tick decode paths).
    pub fn scroll(&mut self, action: Action, bounds: ScrollBounds) -> bool {
        scroll::apply(&mut self.scroll_x, &mut self.scroll_y, action, bounds)
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
    /// effective grid's edges). `ZoomReset` always sends scroll to the
    /// origin regardless of the prior pan.
    pub fn handle_zoom(&mut self, action: Action, bounds: ViewBounds) -> Option<Handled> {
        let new_zoom = match action {
            Action::ZoomIn => self.zoom.step_in(),
            Action::ZoomOut => self.zoom.step_out(),
            Action::ZoomReset => {
                self.zoom = ZoomLevel::one();
                self.reset_pan();
                return Some(Handled::Yes);
            }
            Action::ZoomPreset(n) => ZoomLevel::preset(n),
            _ => return None,
        };
        if new_zoom == self.zoom {
            return Some(Handled::Yes);
        }
        anchor_zoom_change(
            self.zoom.factor(),
            new_zoom.factor(),
            bounds.viewport_cols,
            bounds.viewport_rows,
            &mut self.scroll_x,
            &mut self.scroll_y,
        );
        self.zoom = new_zoom;
        Some(Handled::Yes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut s = ZoomPanState::new();
        s.handle_zoom(Action::ZoomIn, bounds(80, 24));
        assert_eq!((s.scroll_x, s.scroll_y), (10, 3));
        assert!((s.zoom.factor() - 1.25).abs() < 1e-3);
    }

    #[test]
    fn zoom_out_after_zoom_in_returns_to_origin() {
        let mut s = ZoomPanState::new();
        s.handle_zoom(Action::ZoomIn, bounds(80, 24));
        s.handle_zoom(Action::ZoomOut, bounds(80, 24));
        assert!(s.zoom.is_one());
        assert_eq!((s.scroll_x, s.scroll_y), (0, 0));
    }

    #[test]
    fn zoom_reset_clears_scroll_regardless_of_pan() {
        let mut s = ZoomPanState::new();
        s.zoom = ZoomLevel::new(4.0);
        s.scroll_x = 100;
        s.scroll_y = 50;
        s.handle_zoom(Action::ZoomReset, bounds(80, 24));
        assert!(s.zoom.is_one());
        assert_eq!((s.scroll_x, s.scroll_y), (0, 0));
    }

    #[test]
    fn zoom_preset_jumps_to_integer_zoom() {
        let mut s = ZoomPanState::new();
        s.handle_zoom(Action::ZoomPreset(3), bounds(80, 24));
        assert_eq!(s.zoom.factor(), 3.0);
    }

    #[test]
    fn zoom_passes_through_non_zoom_actions() {
        let mut s = ZoomPanState::new();
        let h = s.handle_zoom(Action::CycleFitMode, bounds(80, 24));
        assert!(h.is_none());
    }
}
