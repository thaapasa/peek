//! Single entry point that hands every font consumer raw sfnt bytes.
//! Bare TrueType / OpenType / Collection containers already *are* sfnt
//! and borrow through untouched; WOFF unwraps to an owned buffer. The
//! metadata gather and specimen rasteriser both call this first so
//! neither has to know which container it was handed.

use std::borrow::Cow;

use anyhow::{Result, anyhow};

use crate::types::font::format::FontFormat;
use crate::types::font::woff;

/// Return the sfnt bytes for a font of the given container `format`,
/// decompressing the WOFF / WOFF2 wrapper when present. Borrows for the
/// bare sfnt formats (no copy); allocates only for the web wrappers.
///
/// WOFF1 is unwrapped in-tree on `flate2` (zlib-per-table); WOFF2 — a
/// brotli stream plus a glyf/loca table transform — is delegated to
/// `wuff`, which reconstructs the sfnt.
pub fn decode(bytes: &[u8], format: FontFormat) -> Result<Cow<'_, [u8]>> {
    match format {
        FontFormat::TrueType | FontFormat::OpenType | FontFormat::Collection => {
            Ok(Cow::Borrowed(bytes))
        }
        FontFormat::Woff => Ok(Cow::Owned(woff::decode(bytes)?)),
        FontFormat::Woff2 => wuff::decompress_woff2(bytes)
            .map(Cow::Owned)
            .map_err(|e| anyhow!("WOFF2: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::font::info_gather;

    /// Bare sfnt formats borrow through without copying or mutating.
    #[test]
    fn bare_sfnt_borrows_through() {
        let bytes = b"\x00\x01\x00\x00rest";
        let out = decode(bytes, FontFormat::TrueType).unwrap();
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out.as_ref(), bytes);
    }

    /// WOFF2 unwrap must reconstruct an sfnt whose parsed metadata
    /// matches the TTF the fixture was generated from. Exercises the
    /// brotli + glyf/loca transform path through `wuff`.
    #[test]
    fn woff2_decodes_to_matching_sfnt() {
        let woff2 = std::fs::read("test-data/fonts/sacramento/Sacramento-Regular.woff2")
            .expect("woff2 fixture present");
        let sfnt = decode(&woff2, FontFormat::Woff2).expect("woff2 decode succeeds");

        let from_woff2 = info_gather::gather(&sfnt, FontFormat::Woff2);
        let ttf = std::fs::read("test-data/fonts/sacramento/Sacramento-Regular.ttf")
            .expect("ttf fixture present");
        let from_ttf = info_gather::gather(&ttf, FontFormat::TrueType);

        assert!(
            from_woff2.parse_errors.is_empty(),
            "{:?}",
            from_woff2.parse_errors
        );
        assert_eq!(from_woff2.faces[0].family, "Sacramento");
        assert_eq!(
            from_woff2.faces[0].glyph_count,
            from_ttf.faces[0].glyph_count
        );
        assert_eq!(
            from_woff2.faces[0].codepoint_count,
            from_ttf.faces[0].codepoint_count
        );
    }
}
