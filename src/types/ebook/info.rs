//! Shared ebook info shape. Field set is universal across EPUB
//! (Dublin Core in OPF), MOBI / AZW3 (EXTH headers), and FB2
//! (description block); per-format gather code populates the same
//! struct so the renderer doesn't need to know the source format.

#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub date: Option<String>,
    pub identifier: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EbookStats {
    pub metadata: Metadata,
    pub chapter_count: usize,
}
