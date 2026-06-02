//! Single entry point that hands every font consumer raw sfnt bytes.
//! Bare TrueType / OpenType / Collection containers already *are* sfnt
//! and borrow through untouched; WOFF unwraps to an owned buffer. The
//! metadata gather and specimen rasteriser both call this first so
//! neither has to know which container it was handed.

use std::borrow::Cow;

use anyhow::Result;

use crate::types::font::format::FontFormat;
use crate::types::font::woff;

/// Return the sfnt bytes for a font of the given container `format`,
/// decompressing the WOFF wrapper when present. Borrows for the bare
/// sfnt formats (no copy); allocates only for WOFF.
pub fn decode(bytes: &[u8], format: FontFormat) -> Result<Cow<'_, [u8]>> {
    match format {
        FontFormat::TrueType | FontFormat::OpenType | FontFormat::Collection => {
            Ok(Cow::Borrowed(bytes))
        }
        FontFormat::Woff => Ok(Cow::Owned(woff::decode(bytes)?)),
    }
}
