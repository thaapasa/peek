//! Extract one embedded attachment (`/EmbeddedFiles` entry) from a
//! PDF. Lookup key is the synthetic name produced by
//! [`super::package::Doc::list_embeds`] — the same string the
//! listing-mode surface uses.

use crate::extract::{ExtractError, Extracted, forward_slash_key, sanitize_entry_path};
use crate::input::InputSource;

use super::package;

pub fn extract(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let safe = sanitize_entry_path(key)?;
    let safe_str = forward_slash_key(&safe);

    let doc = package::open_doc(source).map_err(ExtractError::Other)?;
    let bytes = doc
        .read_embed(&safe_str)
        .map_err(|_| ExtractError::NotFound(key.to_string()))?;
    if bytes.is_empty() {
        return Err(ExtractError::Other(anyhow::anyhow!(
            "attachment {safe_str} has empty payload"
        )));
    }
    let suggested = safe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| safe_str.clone());
    Ok(Extracted {
        suggested_name: suggested,
        source: InputSource::Memory {
            bytes,
            name: safe_str,
        },
    })
}
