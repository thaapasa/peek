//! Extension-based comic-archive format detection.

use super::format::ComicFormat;

/// Map a single file extension to a comic-archive format.
pub fn format_from_ext(ext: &str) -> Option<ComicFormat> {
    match ext {
        "cbz" => Some(ComicFormat::Cbz),
        _ => None,
    }
}
