//! Shared document info shape. Field set is universal across DOCX
//! (Office Open XML core properties) and RTF (`\info` group); per-format
//! gather code populates the same struct so the renderer doesn't need
//! to know the source format.

use std::time::SystemTime;

use peek_detect::DocumentFormat;

#[derive(Debug, Clone, Default)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub keywords: Option<String>,
    /// Authoring timestamps, parsed to a wall-clock instant at gather time
    /// (`None` when the source date is absent or unparseable). Rendered muted /
    /// serialized ISO-8601 UTC via [`Value::timestamp`](crate::info::Value).
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
}

impl DocumentMetadata {
    /// Parse an ISO-8601 source date into `created` unless already set —
    /// first non-empty wins. Unparseable input leaves the slot `None`.
    pub fn set_created_iso(&mut self, raw: &str) {
        if self.created.is_none() {
            self.created = crate::info::parse_iso8601(raw);
        }
    }
    /// ISO-8601 counterpart of [`set_created_iso`](Self::set_created_iso) for
    /// `modified`.
    pub fn set_modified_iso(&mut self, raw: &str) {
        if self.modified.is_none() {
            self.modified = crate::info::parse_iso8601(raw);
        }
    }
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
