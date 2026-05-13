//! SVG info shape: text-stats sidecar plus SVG-specific extras
//! (declared dimensions, element counts, security flags, animation
//! summary).

use crate::types::text::info::TextStats;

pub struct SvgStats {
    pub text: TextStats,
    pub view_box: Option<String>,
    pub declared_width: Option<String>,
    pub declared_height: Option<String>,
    pub path_count: usize,
    pub group_count: usize,
    pub rect_count: usize,
    pub circle_count: usize,
    pub text_count: usize,
    pub has_script: bool,
    pub has_external_href: bool,
    pub animation: Option<SvgAnimationStats>,
    /// Set when the SVG declared an animation peek can't play
    /// (unsupported feature, malformed, or rasterization probe
    /// failed). Surfaced as a warning row in the info view.
    pub animation_warning: Option<String>,
}

/// SVG CSS-keyframe animation stats (from `types::image::pipeline::svg_anim`).
pub struct SvgAnimationStats {
    pub frame_count: usize,
    pub total_duration_ms: u64,
    pub infinite: bool,
}
