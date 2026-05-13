//! Word-processing document format enum + display label.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormat {
    /// Office Open XML word-processing document. ZIP container with
    /// `word/document.xml` body + `docProps/*.xml` metadata.
    Docx,
    /// OpenDocument Text. ZIP container with `content.xml` body and
    /// `meta.xml` Dublin Core metadata.
    Odt,
    /// Rich Text Format. Control-word markup; single file, not a
    /// container.
    Rtf,
}

impl DocumentFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Docx => "DOCX document",
            Self::Odt => "ODT document",
            Self::Rtf => "RTF document",
        }
    }
}
