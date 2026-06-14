//! Extract handler for notebook listing rows.
//!
//! The key is the synthetic block name written by [`super::listing`]
//! (`code-1.py` / `image-2.png`). Resolution re-reads the notebook,
//! finds the matching block, and returns an in-memory source named after
//! the block. peek's outer re-detect then picks it up by extension /
//! magic — code opens in the syntax view, images in the ASCII renderer —
//! so no notebook-specific descend logic is needed.

use anyhow::anyhow;

use crate::extract::{ExtractError, Extracted};
use peek_io::InputSource;

use super::listing;

pub fn extract(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let text = source
        .read_text(peek_io::limits::Budget::WholeDoc("notebook"))
        .map_err(ExtractError::Other)?;
    let (name, bytes) = listing::extract_block(&text, key)
        .ok_or_else(|| ExtractError::NotFound(key.to_string()))?;
    if bytes.is_empty() {
        return Err(ExtractError::Other(anyhow!(
            "block {key} decoded to no bytes"
        )));
    }
    Ok(Extracted {
        source: InputSource::memory(bytes, name.clone()),
        suggested_name: name,
    })
}
