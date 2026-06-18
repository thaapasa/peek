//! Shared presentation info shape. Field set is universal across PPTX
//! (Office Open XML core/app properties), ODP (`meta.xml`), and Keynote
//! (`Metadata/BuildVersionHistory.plist`); per-format gather code
//! populates the same struct so the renderer doesn't need to know the
//! source format.

use std::time::SystemTime;

use peek_detect::PresentationFormat;

#[derive(Debug, Clone, Default)]
pub struct PresentationMetadata {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    /// Authoring timestamps, parsed to a wall-clock instant at gather time
    /// (`None` when the source date is absent or unparseable). Rendered muted /
    /// serialized ISO-8601 UTC via [`Value::timestamp`](crate::info::Value).
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    /// Creating application — PPTX `docProps/app.xml` `<Application>`,
    /// Keynote `BuildVersionHistory.plist`. Best-effort.
    pub application: Option<String>,
}

impl PresentationMetadata {
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
pub struct PresentationStats {
    pub format: PresentationFormat,
    pub metadata: PresentationMetadata,
    pub slide_count: usize,
    pub word_count: usize,
    /// Embedded image references across all slides. Always 0 for
    /// Keynote (slide bodies aren't parsed).
    pub image_count: usize,
}

impl PresentationStats {
    pub fn empty(format: PresentationFormat) -> Self {
        Self {
            format,
            metadata: PresentationMetadata::default(),
            slide_count: 0,
            word_count: 0,
            image_count: 0,
        }
    }
}
