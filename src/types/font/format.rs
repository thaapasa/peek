//! Font container format. Covers the bare OpenType wrappers shipped by
//! the desktop ecosystem. WOFF / WOFF2 are tracked in
//! `docs/planned.md` — they need a separate decompression dependency
//! (zlib for WOFF, brotli for WOFF2) and are deferred.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFormat {
    /// TrueType outline font (`.ttf`). Magic: `00 01 00 00`.
    TrueType,
    /// OpenType / CFF outline font (`.otf`). Magic: `OTTO`.
    OpenType,
    /// TrueType / OpenType font collection (`.ttc` / `.otc`). One
    /// container, many faces. Magic: `ttcf`.
    Collection,
}

impl FontFormat {
    pub fn label(self) -> &'static str {
        match self {
            FontFormat::TrueType => "TrueType",
            FontFormat::OpenType => "OpenType",
            FontFormat::Collection => "Font Collection",
        }
    }
}
