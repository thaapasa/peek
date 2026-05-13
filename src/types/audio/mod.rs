//! Audio files: metadata-only info view + embed listing driven by
//! symphonia. The shared [`package::probe`] resolves container / codec
//! / channels / bit depth / sample rate + tag fields and surfaces
//! embedded pictures + lyrics; the listing TOC and `--extract` reuse
//! the same probe result via [`package::build_listing`] /
//! [`package::read_embed`]. No playback, no waveform.

pub mod compose;
pub mod detect;
pub mod extract;
pub mod format;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod package;

pub use info::AudioStats;
