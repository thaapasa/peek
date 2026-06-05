//! Gather RTF-specific extras for the Info section.

use crate::info::Extras;
use crate::input::InputSource;
use crate::input::detect::DocumentFormat;
use crate::types::document::DocumentStats;

use super::parse;

pub fn gather_extras(source: &InputSource) -> Extras {
    match parse::open_source(source) {
        Ok(parsed) => Box::new(DocumentStats {
            format: DocumentFormat::Rtf,
            metadata: parsed.metadata,
            paragraph_count: parsed.paragraph_count,
            word_count: parsed.word_count,
            image_count: parsed.embeds.len(),
        }),
        Err(_) => Box::new(DocumentStats::empty(DocumentFormat::Rtf)),
    }
}
