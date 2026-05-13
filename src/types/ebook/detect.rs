//! Extension-based e-book format detection.

use super::format::EbookFormat;

/// Map a single file extension to an e-book format.
pub fn format_from_ext(ext: &str) -> Option<EbookFormat> {
    match ext {
        "epub" => Some(EbookFormat::Epub),
        _ => None,
    }
}
