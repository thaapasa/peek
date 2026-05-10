//! Gather RTF-specific extras for the Info section.

use crate::info::FileExtras;
use crate::input::InputSource;
use crate::input::detect::DocumentFormat;
use crate::types::document::DocumentStats;

use super::parse;

pub fn gather_extras(source: &InputSource) -> FileExtras {
    match parse::open_source(source) {
        Ok(parsed) => FileExtras::Document(DocumentStats {
            format: DocumentFormat::Rtf,
            metadata: parsed.metadata,
            paragraph_count: parsed.paragraph_count,
            word_count: parsed.word_count,
            image_count: parsed.embeds.len(),
        }),
        Err(_) => FileExtras::Document(DocumentStats::empty(DocumentFormat::Rtf)),
    }
}
