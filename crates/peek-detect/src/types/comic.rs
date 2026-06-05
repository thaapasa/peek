//! Comic-archive container format enum + display label.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComicFormat {
    /// Comic Book ZIP (most common comic-archive form in the wild).
    Cbz,
}

impl ComicFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cbz => "Comic Book ZIP",
        }
    }
}

// Extension-based comic-archive format detection.

/// Map a single file extension to a comic-archive format.
pub fn format_from_ext(ext: &str) -> Option<ComicFormat> {
    match ext {
        "cbz" => Some(ComicFormat::Cbz),
        _ => None,
    }
}
