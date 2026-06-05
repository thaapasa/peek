//! Extract one embedded item (picture / lyrics) from an audio file.
//! Lookup key is the synthetic path produced by
//! [`super::package::build_listing`]; the listing surface, `--list`,
//! and `--extract` all use the same string. Returned source is in-
//! memory bytes that re-enter the peek pipeline (image bytes
//! re-detect as Image and route through the ASCII pipeline; lyrics
//! re-detect as plain text).

use crate::extract::{ExtractError, Extracted, forward_slash_key, sanitize_entry_path};
use crate::input::InputSource;
use crate::input::detect::AudioFormat;

use super::package;

pub fn extract(
    source: &InputSource,
    format: AudioFormat,
    key: &str,
) -> Result<Extracted, ExtractError> {
    let safe = sanitize_entry_path(key)?;
    let safe_str = forward_slash_key(&safe);

    let probed = package::probe(source, format).map_err(ExtractError::Other)?;
    let (bytes, suggested) = package::read_embed(&probed, &safe_str)
        .ok_or_else(|| ExtractError::NotFound(key.to_string()))?;
    if bytes.is_empty() {
        return Err(ExtractError::Other(anyhow::anyhow!(
            "embed {safe_str} has empty payload"
        )));
    }
    Ok(Extracted {
        suggested_name: suggested,
        source: InputSource::Memory {
            bytes,
            name: safe_str,
        },
    })
}
