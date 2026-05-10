//! Extract one embedded resource (`\pict` / `\object` group) from
//! an RTF file. The lookup key is the synthetic name surfaced by
//! [`crate::types::document::rtf::parse::embeds_to_entries`] —
//! e.g. `image1.jpg` for the first picture group.
//!
//! Decoded bytes return as an in-memory [`InputSource`] so the
//! existing recursive-peek pipeline can re-detect them (a `.jpg`
//! payload classifies as `FileType::Image` on the next pass).

use bytes::Bytes;

use crate::extract::{ExtractError, Extracted};
use crate::input::InputSource;

use super::parse;

pub fn extract(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let parsed = parse::open_source(source).map_err(ExtractError::Other)?;
    let embed = parsed
        .embeds
        .iter()
        .find(|e| e.name == key)
        .ok_or_else(|| ExtractError::NotFound(key.to_string()))?;
    let bytes = embed.decode_bytes();
    if bytes.is_empty() {
        return Err(ExtractError::Other(anyhow::anyhow!(
            "embed {} has no decoded payload",
            embed.name
        )));
    }
    Ok(Extracted {
        suggested_name: embed.name.clone(),
        source: InputSource::Memory {
            bytes: Bytes::from(bytes),
            name: embed.name.clone(),
        },
    })
}
