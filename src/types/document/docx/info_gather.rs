//! Gather DOCX-specific extras for the Info section.

use crate::info::FileExtras;
use crate::input::InputSource;
use crate::input::detect::DocumentFormat;
use crate::types::document::DocumentStats;

use super::package;

pub fn gather_extras(source: &InputSource) -> FileExtras {
    match package::open(source) {
        Ok(doc) => FileExtras::Document(DocumentStats {
            format: DocumentFormat::Docx,
            metadata: doc.metadata,
            paragraph_count: doc.paragraph_count,
            word_count: doc.word_count,
            image_count: doc.image_count,
        }),
        Err(_) => FileExtras::Document(DocumentStats::empty(DocumentFormat::Docx)),
    }
}
