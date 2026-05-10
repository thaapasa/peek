//! Shared PDF info shape. Reuses [`DocumentMetadata`] from
//! `types::document::info` so PDF surfaces title / creator / subject /
//! keywords / created / modified through the same renderer chain as
//! DOCX and RTF.

use crate::types::document::DocumentMetadata;

#[derive(Debug, Clone)]
pub struct PdfStats {
    pub metadata: DocumentMetadata,
    pub page_count: usize,
    /// Count of `/EmbeddedFiles` attachments (file streams attached
    /// at the document level — invoices, source data, etc).
    pub attachment_count: usize,
    /// Count of inline image XObjects across all pages (the rasters
    /// drawn by the page-content stream — photos, screenshots,
    /// figures).
    pub image_count: usize,
    pub encrypted: bool,
    /// PDF version string from the file header (e.g. "1.7"). Empty when
    /// the parser can't read it (corrupt header, encrypted with no
    /// password access).
    pub pdf_version: String,
    /// User-facing reason the document couldn't be opened (encrypted
    /// without a password, corrupt, missing pdfium library). When
    /// present, the rest of the fields are zero / default and the
    /// info-render shows this string instead of stats.
    pub error: Option<String>,
}

impl PdfStats {
    pub fn empty() -> Self {
        Self {
            metadata: DocumentMetadata::default(),
            page_count: 0,
            attachment_count: 0,
            image_count: 0,
            encrypted: false,
            pdf_version: String::new(),
            error: None,
        }
    }
}
