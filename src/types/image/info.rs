//! Image statistics shape: dimensions, color characteristics, ICC, HDR
//! marker, animation summary, and EXIF / XMP key-value pairs.

pub struct ImageStats {
    pub width: u32,
    pub height: u32,
    pub color_type: String,
    pub bit_depth: u8,
    pub hdr_format: Option<String>,
    pub icc_profile: Option<String>,
    pub animation: Option<AnimationStats>,
    pub exif: Vec<(String, String)>,
    pub xmp: Vec<(String, String)>,
}

/// Animation playback stats. Counts/durations may be `None` when the format
/// requires full decoding to compute (WebP) — the cheap header-walk path is
/// only available for GIF.
pub struct AnimationStats {
    pub frame_count: Option<usize>,
    pub total_duration_ms: Option<u64>,
    pub loop_count: Option<LoopCount>,
}

#[derive(Debug, Clone, Copy)]
pub enum LoopCount {
    Infinite,
    Finite(u32),
}
