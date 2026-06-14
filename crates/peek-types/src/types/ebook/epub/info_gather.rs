//! Gather EPUB-specific extras for the Info section.

use crate::info::Extras;
use crate::types::ebook::EbookStats;
use peek_io::InputSource;

use super::package;

pub fn gather_extras(source: &InputSource) -> Extras {
    match package::open(source) {
        Ok(pkg) => Box::new(EbookStats {
            metadata: pkg.metadata,
            chapter_count: pkg.chapters.len(),
        }),
        Err(_) => Box::new(EbookStats::default()),
    }
}
