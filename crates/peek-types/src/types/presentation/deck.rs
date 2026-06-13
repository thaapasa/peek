//! Parsed presentation deck: one [`Doc`] per slide.
//!
//! Reuses the shared word-processing AST ([`crate::types::document::ast`])
//! as the per-slide prose model so the slide read view can lean on the
//! existing document renderer — a slide's title + bullets are just a
//! heading paragraph followed by body paragraphs. The PPTX and ODP
//! parsers each emit a `Deck`; the Keynote path doesn't (its slide text
//! isn't parsed).

use crate::types::document::ast::Doc;

use super::PresentationMetadata;

/// A fully parsed deck. `slides[i]` is the prose of slide `i + 1`,
/// rendered through `crate::types::document::render::render`.
pub struct Deck {
    pub metadata: PresentationMetadata,
    pub slides: Vec<Doc>,
}

impl Deck {
    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    /// Total words across every slide.
    pub fn word_count(&self) -> usize {
        self.slides.iter().map(|d| d.word_count).sum()
    }

    /// Total embedded-image references across every slide.
    pub fn image_count(&self) -> usize {
        self.slides.iter().map(|d| d.image_count).sum()
    }
}
