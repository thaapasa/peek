//! E-book container format enum.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EbookFormat {
    /// EPUB — ZIP container with HTML chapters + OPF metadata.
    Epub,
}

// Extension-based e-book format detection.

/// Map a single file extension to an e-book format.
pub fn format_from_ext(ext: &str) -> Option<EbookFormat> {
    match ext {
        "epub" => Some(EbookFormat::Epub),
        _ => None,
    }
}
