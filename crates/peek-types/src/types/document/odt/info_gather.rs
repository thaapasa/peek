//! Gather ODT-specific extras for the Info section.

use peek_detect::DocumentFormat;
use peek_io::InputSource;

use super::package;
use crate::info::Extras;
use crate::types::document::DocumentStats;

pub fn gather_extras(source: &InputSource) -> Extras {
    match package::open(source) {
        Ok(doc) => Box::new(DocumentStats {
            format: DocumentFormat::Odt,
            metadata: doc.metadata,
            paragraph_count: doc.paragraph_count,
            word_count: doc.word_count,
            image_count: doc.image_count,
        }),
        Err(_) => Box::new(DocumentStats::empty(DocumentFormat::Odt)),
    }
}
