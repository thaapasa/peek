//! Gather DOCX-specific extras for the Info section.

use crate::info::Extras;
use crate::input::InputSource;
use crate::input::detect::DocumentFormat;
use crate::types::document::DocumentStats;

use super::package;

pub fn gather_extras(source: &InputSource) -> Extras {
    match package::open(source) {
        Ok(doc) => Box::new(DocumentStats {
            format: DocumentFormat::Docx,
            metadata: doc.metadata,
            paragraph_count: doc.paragraph_count,
            word_count: doc.word_count,
            image_count: doc.image_count,
        }),
        Err(_) => Box::new(DocumentStats::empty(DocumentFormat::Docx)),
    }
}
