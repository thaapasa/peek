//! PostScript-family format: Encapsulated PostScript (`.eps`) vs plain
//! PostScript (`.ps`). The two share the whole viewer (embedded preview,
//! Ghostscript render, source, DSC info); the format only labels the
//! Info section and decides whether Ghostscript renders with `-dEPSCrop`
//! (crop to the EPS BoundingBox) or the full default page.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostScriptFormat {
    /// Encapsulated PostScript — single illustration with a
    /// `%%BoundingBox`, optionally a binary preview header.
    Eps,
    /// Plain PostScript program / document.
    Ps,
}

impl PostScriptFormat {
    /// Human label for the Info section header.
    pub fn label(self) -> &'static str {
        match self {
            PostScriptFormat::Eps => "EPS",
            PostScriptFormat::Ps => "PostScript",
        }
    }

    /// Whether Ghostscript should crop to the EPS BoundingBox.
    pub fn crop_to_bbox(self) -> bool {
        matches!(self, PostScriptFormat::Eps)
    }
}
