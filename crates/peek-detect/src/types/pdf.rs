//! PDF-family flavour. Adobe Illustrator `.ai` files are PDF 1.x
//! internally — Illustrator's default "Create PDF Compatible File" save
//! embeds a full PDF rendering, so they open and render through the
//! exact same Pdfium pipeline as a plain PDF. The flavour only drives
//! the Info section label and the extension-mismatch allow-list (so a
//! `.ai` extension over `%PDF` magic doesn't read as a lie); it never
//! changes the render path.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfFlavor {
    /// Plain PDF document.
    Pdf,
    /// Adobe Illustrator artwork saved PDF-compatible (`.ai`).
    Illustrator,
}

impl PdfFlavor {
    /// Human label for the Info section header.
    pub fn label(self) -> &'static str {
        match self {
            PdfFlavor::Pdf => "PDF",
            PdfFlavor::Illustrator => "Adobe Illustrator",
        }
    }
}
