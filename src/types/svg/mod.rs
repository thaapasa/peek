//! SVG support: vector image with dual nature (rasterized preview +
//! XML source). The static-render path reuses `types::image` for
//! rasterization; the animation path uses `types::image::pipeline::svg_anim`
//! for the CSS-keyframe parser/timeline plus this module's
//! `SvgAnimationMode` for playback (per-frame rasterize + LRU cache).
//!
//! `info_gather` collects SVG-specific extras (viewBox, declared
//! dimensions, element counts, security flags, animation summary) on
//! top of the underlying text stats; `info_render` draws the SVG info
//! section.

pub mod animation_mode;
pub mod info_gather;
pub mod info_render;

pub(crate) use animation_mode::SvgAnimationMode;
