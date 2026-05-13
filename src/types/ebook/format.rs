//! E-book container format enum.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EbookFormat {
    /// EPUB — ZIP container with HTML chapters + OPF metadata.
    Epub,
}
