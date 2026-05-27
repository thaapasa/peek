//! Zoom state for graphic-rendering viewer modes.
//!
//! A zoom level multiplies the base cell grid (computed at fit-mode
//! Contain / FitWidth / FitHeight). Effective grid = base × zoom; the
//! visible viewport is the terminal-sized window into that effective
//! grid, so panning scales linearly with zoom.
//!
//! Rendering at zoom > 1 must crop the visible pixel ROI from the
//! native-resolution source and rescale only that ROI to viewport
//! pixels — never resize the full effective grid, which would blow
//! memory quadratically with zoom.

use std::cmp::Ordering;

/// Multiplicative zoom factor applied on top of the current fit-mode
/// base grid. `1.0` = fit-natural, `2.0` = displayed at twice the size
/// along each axis, etc.
#[derive(Copy, Clone, Debug)]
pub(crate) struct ZoomLevel(f32);

impl ZoomLevel {
    /// Smallest representable zoom — also the fit-natural baseline.
    /// Zooming below 1 isn't meaningful: fit mode already constrains
    /// the image to the smallest sensible display size.
    pub const MIN: f32 = 1.0;
    /// Largest zoom. Beyond ~16× each glyph cell maps to a pixel blob
    /// in the source so further zoom adds no detail.
    pub const MAX: f32 = 16.0;
    /// Multiplicative step per `+` / `-` press.
    pub const STEP: f32 = 1.25;

    pub const fn one() -> Self {
        Self(1.0)
    }

    pub fn new(factor: f32) -> Self {
        Self(factor.clamp(Self::MIN, Self::MAX))
    }

    pub fn factor(self) -> f32 {
        self.0
    }

    pub fn is_one(self) -> bool {
        self.0 <= 1.0
    }

    pub fn step_in(self) -> Self {
        Self::new(self.0 * Self::STEP)
    }

    pub fn step_out(self) -> Self {
        Self::new(self.0 / Self::STEP)
    }

    /// Map a 1-9 preset key to a whole-number zoom level (1× .. 9×).
    /// Out-of-range values clamp to MIN..=MAX.
    pub fn preset(n: u8) -> Self {
        Self::new(n as f32)
    }

    /// Format for the status line. `1.0` renders as `1×`, `2.5` as
    /// `2.5×`; one decimal place when non-integer, trimmed when whole.
    pub fn label(self) -> String {
        let f = self.0;
        if (f - f.round()).abs() < 0.05 {
            format!("{}×", f.round() as u32)
        } else {
            format!("{f:.2}×")
        }
    }
}

impl Default for ZoomLevel {
    fn default() -> Self {
        Self::one()
    }
}

impl PartialEq for ZoomLevel {
    fn eq(&self, other: &Self) -> bool {
        // ~1e-3 tolerance is well below any user-visible step (1.25×).
        (self.0 - other.0).abs() < 1e-3
    }
}

impl Eq for ZoomLevel {}

impl PartialOrd for ZoomLevel {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ZoomLevel {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}

/// Snapshot of a zoom-aware viewport: base cell grid, zoom factor,
/// terminal dimensions. Derives every other quantity (effective grid,
/// visible viewport, pan bounds, pixel ROI) so the same formula lives
/// in one place — the cell-projection used by the renderer and the
/// pan-bounds used by `ImageView::view_bounds` cannot drift.
#[derive(Copy, Clone, Debug)]
pub(crate) struct ZoomedView {
    pub base_cols: u32,
    pub base_rows: u32,
    pub term_cols: u32,
    pub term_rows: u32,
    pub zoom: f32,
}

/// Pixel rectangle in source-image coordinates.
#[derive(Copy, Clone, Debug)]
pub(crate) struct PixelRoi {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl ZoomedView {
    /// Cell grid at the current zoom: `(base × zoom)` rounded, each
    /// axis floored at 1.
    pub fn effective(&self) -> (u32, u32) {
        let z = self.zoom.max(1.0);
        let cols = ((self.base_cols as f32 * z).round() as u32).max(1);
        let rows = ((self.base_rows as f32 * z).round() as u32).max(1);
        (cols, rows)
    }

    /// Visible viewport in cells = `min(effective, terminal)`, each
    /// axis floored at 1.
    pub fn viewport(&self) -> (u32, u32) {
        let (ec, er) = self.effective();
        (ec.min(self.term_cols).max(1), er.min(self.term_rows).max(1))
    }

    /// How far the scroll origin can move on each axis while keeping
    /// the viewport on the effective grid.
    pub fn max_scroll(&self) -> (u32, u32) {
        let (ec, er) = self.effective();
        let (vc, vr) = self.viewport();
        (ec.saturating_sub(vc), er.saturating_sub(vr))
    }

    /// Clamp scroll offsets in place against `max_scroll`.
    pub fn clamp_scroll(&self, sx: &mut u32, sy: &mut u32) {
        let (mx, my) = self.max_scroll();
        *sx = (*sx).min(mx);
        *sy = (*sy).min(my);
    }

    /// Source-pixel rectangle that the visible viewport maps to at
    /// the given scroll offset. `src_w`/`src_h` are the source image
    /// dimensions in pixels; the rect is clamped to the source bounds
    /// and width / height floor at 1.
    pub fn pixel_roi(&self, src_w: u32, src_h: u32, scroll_x: u32, scroll_y: u32) -> PixelRoi {
        let (ec, er) = self.effective();
        let (vc, vr) = self.viewport();
        let src_w = src_w.max(1);
        let src_h = src_h.max(1);
        let to_x =
            |c: u32| -> u32 { ((c as u64 * src_w as u64) / ec as u64).min(src_w as u64) as u32 };
        let to_y =
            |r: u32| -> u32 { ((r as u64 * src_h as u64) / er as u64).min(src_h as u64) as u32 };
        let x0 = to_x(scroll_x);
        let x1 = to_x(scroll_x + vc);
        let y0 = to_y(scroll_y);
        let y1 = to_y(scroll_y + vr);
        PixelRoi {
            x: x0,
            y: y0,
            w: x1.saturating_sub(x0).max(1),
            h: y1.saturating_sub(y0).max(1),
        }
    }
}

/// Apply the "viewport-centre pixel stays put" anchor to a scroll
/// position when zoom transitions from `old_zoom` to `new_zoom`.
/// `viewport_cols` / `viewport_rows` describe the visible viewport
/// at the *old* zoom — the centre cell in those coordinates is the
/// fixed point. Caller assigns the new zoom value separately.
pub(crate) fn anchor_zoom_change(
    old_zoom: f32,
    new_zoom: f32,
    viewport_cols: u32,
    viewport_rows: u32,
    scroll_x: &mut u32,
    scroll_y: &mut u32,
) {
    if old_zoom <= 0.0 {
        return;
    }
    let half_w = viewport_cols as f32 / 2.0;
    let half_h = viewport_rows as f32 / 2.0;
    let cx = (*scroll_x as f32 + half_w) * (new_zoom / old_zoom);
    let cy = (*scroll_y as f32 + half_h) * (new_zoom / old_zoom);
    *scroll_x = (cx - half_w).max(0.0).round() as u32;
    *scroll_y = (cy - half_h).max(0.0).round() as u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_in_and_out_invert_within_clamp() {
        let z = ZoomLevel::new(2.0);
        assert_eq!(z.step_in().step_out(), z);
    }

    #[test]
    fn step_out_clamps_at_min() {
        assert_eq!(ZoomLevel::one().step_out(), ZoomLevel::one());
    }

    #[test]
    fn step_in_clamps_at_max() {
        let mut z = ZoomLevel::new(ZoomLevel::MAX);
        for _ in 0..5 {
            z = z.step_in();
        }
        assert!((z.factor() - ZoomLevel::MAX).abs() < 1e-3);
    }

    #[test]
    fn preset_maps_digit_to_integer_zoom() {
        for n in 1u8..=9 {
            assert_eq!(ZoomLevel::preset(n).factor(), n as f32);
        }
    }

    #[test]
    fn preset_above_max_clamps() {
        assert_eq!(ZoomLevel::preset(99).factor(), ZoomLevel::MAX);
    }

    #[test]
    fn label_renders_integer_zoom_without_decimal() {
        assert_eq!(ZoomLevel::one().label(), "1×");
        assert_eq!(ZoomLevel::preset(3).label(), "3×");
    }

    #[test]
    fn label_renders_fractional_zoom_with_two_decimals() {
        assert_eq!(ZoomLevel::new(1.25).label(), "1.25×");
    }

    #[test]
    fn zoomed_view_effective_scales_with_zoom() {
        let zv = ZoomedView {
            base_cols: 40,
            base_rows: 20,
            term_cols: 80,
            term_rows: 40,
            zoom: 2.0,
        };
        assert_eq!(zv.effective(), (80, 40));
    }

    #[test]
    fn zoomed_view_viewport_clamps_to_terminal() {
        // Effective 160×80 > terminal 80×40 → viewport == terminal.
        let zv = ZoomedView {
            base_cols: 80,
            base_rows: 40,
            term_cols: 80,
            term_rows: 40,
            zoom: 2.0,
        };
        assert_eq!(zv.viewport(), (80, 40));
        assert_eq!(zv.max_scroll(), (80, 40));
    }

    #[test]
    fn zoomed_view_viewport_caps_at_effective_when_smaller_than_terminal() {
        // Effective 30×20 < terminal 80×40 → viewport == effective, no pan.
        let zv = ZoomedView {
            base_cols: 30,
            base_rows: 20,
            term_cols: 80,
            term_rows: 40,
            zoom: 1.0,
        };
        assert_eq!(zv.viewport(), (30, 20));
        assert_eq!(zv.max_scroll(), (0, 0));
    }

    #[test]
    fn zoomed_view_clamp_scroll_pins_overshoot_to_max() {
        let zv = ZoomedView {
            base_cols: 80,
            base_rows: 40,
            term_cols: 80,
            term_rows: 40,
            zoom: 2.0,
        };
        let (mut sx, mut sy) = (9999, 9999);
        zv.clamp_scroll(&mut sx, &mut sy);
        assert_eq!((sx, sy), (80, 40));
    }

    #[test]
    fn zoomed_view_pixel_roi_starts_at_origin_for_zero_scroll() {
        let zv = ZoomedView {
            base_cols: 80,
            base_rows: 40,
            term_cols: 80,
            term_rows: 40,
            zoom: 2.0,
        };
        // Source pixel grid 1600×800; effective 160×80; viewport 80×40.
        // At scroll (0,0): pixel ROI is (0,0) to (1600 * 80/160, 800 * 40/80) = (800,400).
        let roi = zv.pixel_roi(1600, 800, 0, 0);
        assert_eq!((roi.x, roi.y, roi.w, roi.h), (0, 0, 800, 400));
    }

    #[test]
    fn zoomed_view_pixel_roi_shifts_with_scroll() {
        let zv = ZoomedView {
            base_cols: 80,
            base_rows: 40,
            term_cols: 80,
            term_rows: 40,
            zoom: 2.0,
        };
        // scroll_x=80 → x0 = 80 * 1600/160 = 800.
        let roi = zv.pixel_roi(1600, 800, 80, 40);
        assert_eq!((roi.x, roi.y, roi.w, roi.h), (800, 400, 800, 400));
    }

    #[test]
    fn anchor_keeps_viewport_centre_pixel_fixed() {
        // Viewport 80×24, scroll (0,0), zoom 1 → 1.25.
        // centre_x = (0 + 40) * 1.25 = 50; new scroll_x = 50 - 40 = 10.
        // centre_y = (0 + 12) * 1.25 = 15; new scroll_y = 15 - 12 = 3.
        let (mut sx, mut sy) = (0u32, 0u32);
        anchor_zoom_change(1.0, 1.25, 80, 24, &mut sx, &mut sy);
        assert_eq!((sx, sy), (10, 3));
    }

    #[test]
    fn anchor_zoom_out_inverse_of_in_at_origin() {
        let (mut sx, mut sy) = (0u32, 0u32);
        anchor_zoom_change(1.0, 1.25, 80, 24, &mut sx, &mut sy);
        anchor_zoom_change(1.25, 1.0, 80, 24, &mut sx, &mut sy);
        assert_eq!((sx, sy), (0, 0));
    }
}
