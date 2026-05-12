//! Audio files: metadata-only info view driven by symphonia. The probe
//! resolves container / codec / channels / bit depth / sample rate +
//! tag fields (title / artist / album / etc); no playback, no waveform.

pub mod info;
pub mod info_gather;
pub mod info_render;

pub use info::AudioStats;
