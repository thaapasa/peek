//! Gather EPUB-specific extras for the Info section.

use crate::info::FileExtras;
use crate::input::InputSource;

use super::package::{self, Metadata};

#[derive(Debug, Clone, Default)]
pub struct EpubStats {
    pub metadata: Metadata,
    pub chapter_count: usize,
}

pub fn gather_extras(source: &InputSource) -> FileExtras {
    match package::open(source) {
        Ok(pkg) => FileExtras::Epub(EpubStats {
            metadata: pkg.metadata,
            chapter_count: pkg.chapters.len(),
        }),
        Err(_) => FileExtras::Epub(EpubStats::default()),
    }
}
