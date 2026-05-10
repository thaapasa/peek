//! Gather EPUB-specific extras for the Info section.

use crate::info::FileExtras;
use crate::input::InputSource;
use crate::types::ebook::EbookStats;

use super::package;

pub fn gather_extras(source: &InputSource) -> FileExtras {
    match package::open(source) {
        Ok(pkg) => FileExtras::Ebook(EbookStats {
            metadata: pkg.metadata,
            chapter_count: pkg.chapters.len(),
        }),
        Err(_) => FileExtras::Ebook(EbookStats::default()),
    }
}
