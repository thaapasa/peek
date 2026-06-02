//! Font container format. Covers the bare OpenType wrappers shipped by
//! the desktop ecosystem plus the WOFF web wrapper. The bare wrappers
//! are raw sfnt; WOFF zlib-compresses each table and is unwrapped to
//! sfnt before the metadata / specimen pipeline sees it (see
//! [`crate::types::font::sfnt`]). WOFF2 (brotli, whole-font transform)
//! is tracked in `docs/planned.md`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFormat {
    /// TrueType outline font (`.ttf`). Magic: `00 01 00 00`.
    TrueType,
    /// OpenType / CFF outline font (`.otf`). Magic: `OTTO`.
    OpenType,
    /// TrueType / OpenType font collection (`.ttc` / `.otc`). One
    /// container, many faces. Magic: `ttcf`.
    Collection,
    /// Web Open Font Format 1.0 (`.woff`). A zlib-per-table wrapper
    /// around an sfnt; magic `wOFF`. Decoded to the inner sfnt before
    /// parsing.
    Woff,
}

impl FontFormat {
    pub fn label(self) -> &'static str {
        match self {
            FontFormat::TrueType => "TrueType",
            FontFormat::OpenType => "OpenType",
            FontFormat::Collection => "Font Collection",
            FontFormat::Woff => "WOFF",
        }
    }
}
