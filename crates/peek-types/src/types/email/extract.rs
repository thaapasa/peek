//! Extract one MIME attachment from an email message. The lookup key is
//! produced by [`message::attachment_keys`] over
//! [`message::attachment_parts`] — the same shared derivation
//! the attachments listing uses — so a row round-trips back to its part.

use bytes::Bytes;
use mail_parser::MessageParser;
use peek_io::InputSource;

use super::message;
use crate::extract::{ExtractError, Extracted, sanitize_entry_path};

pub fn extract(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let bytes = source
        .read_bytes(peek_io::limits::Budget::Sidecar("email"))
        .map_err(ExtractError::Other)?;
    let msg = MessageParser::default()
        .parse(bytes.as_ref())
        .ok_or_else(|| ExtractError::Other(anyhow::anyhow!("could not parse email")))?;

    // Same parts + key derivation the listing used (shared helpers), so a
    // row round-trips to its part.
    let parts = message::attachment_parts(&msg);
    let keys = message::attachment_keys(&parts);
    let Some(idx) = keys.iter().position(|k| k == key) else {
        return Err(ExtractError::NotFound(key.to_string()));
    };
    // The key is already a plain filename; sanitise guards against path
    // traversal in a declared name and yields the suggested download name.
    let safe = sanitize_entry_path(key)?;
    let suggested = safe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| key.to_string());
    Ok(Extracted {
        suggested_name: suggested,
        source: InputSource::Memory {
            bytes: Bytes::copy_from_slice(&parts[idx].decoded()),
            name: key.to_string(),
        },
    })
}
