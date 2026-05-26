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
}
