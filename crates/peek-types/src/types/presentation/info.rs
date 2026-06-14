//! Shared presentation info shape. Field set is universal across PPTX
//! (Office Open XML core/app properties), ODP (`meta.xml`), and Keynote
//! (`Metadata/BuildVersionHistory.plist`); per-format gather code
//! populates the same struct so the renderer doesn't need to know the
//! source format.

use peek_detect::PresentationFormat;

#[derive(Debug, Clone, Default)]
pub struct PresentationMetadata {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
    /// Creating application — PPTX `docProps/app.xml` `<Application>`,
    /// Keynote `BuildVersionHistory.plist`. Best-effort.
    pub application: Option<String>,
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
