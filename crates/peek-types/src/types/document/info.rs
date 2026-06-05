//! Shared document info shape. Field set is universal across DOCX
//! (Office Open XML core properties) and RTF (`\info` group); per-format
//! gather code populates the same struct so the renderer doesn't need
//! to know the source format.

use crate::input::detect::DocumentFormat;

#[derive(Debug, Clone, Default)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub keywords: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DocumentStats {
    pub format: DocumentFormat,
    pub metadata: DocumentMetadata,
    pub paragraph_count: usize,
    pub word_count: usize,
    /// Number of embedded images. RTF: always 0 (image extraction not
    /// supported); DOCX: walks `word/media/*` entries.
    pub image_count: usize,
}

impl DocumentStats {
    pub fn empty(format: DocumentFormat) -> Self {
        Self {
            format,
            metadata: DocumentMetadata::default(),
            paragraph_count: 0,
            word_count: 0,
            image_count: 0,
        }
    }
}
