//! Font container format. Covers the bare OpenType wrappers shipped by
//! the desktop ecosystem plus the WOFF / WOFF2 web wrappers. The bare
//! wrappers are raw sfnt; WOFF zlib-compresses each table and WOFF2
//! brotli-compresses the whole font with a glyf/loca transform. Both
//! are unwrapped to sfnt before the metadata / specimen pipeline sees
//! them (see [`crate::types::font::sfnt`]).

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
    /// Web Open Font Format 2.0 (`.woff2`). Brotli-compressed whole
    /// font with a glyf/loca table transform; magic `wOF2`. Decoded to
    /// the inner sfnt before parsing.
    Woff2,
}

impl FontFormat {
    pub fn label(self) -> &'static str {
        match self {
            FontFormat::TrueType => "TrueType",
            FontFormat::OpenType => "OpenType",
            FontFormat::Collection => "Font Collection",
            FontFormat::Woff => "WOFF",
            FontFormat::Woff2 => "WOFF2",
        }
    }
}
