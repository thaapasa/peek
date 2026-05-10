//! Shared comic-archive info shape. Same layout across CBZ / CBR /
//! CB7 / CBT containers; per-format gather code populates this struct
//! so the renderer doesn't need to know the source format.

use crate::input::detect::ComicFormat;

#[derive(Debug, Clone)]
pub struct ComicStats {
    pub format: ComicFormat,
    pub page_count: usize,
    /// Total uncompressed size of image entries (sum of all image
    /// pages). `None` when the listing failed and stats are absent.
    pub total_image_bytes: u64,
}
