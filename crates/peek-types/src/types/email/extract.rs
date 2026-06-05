//! Extract one MIME attachment from an email message. The lookup key is
//! produced by [`message::attachment_keys`] — the same shared derivation
//! the attachments listing uses — so a row round-trips back to its part.

use bytes::Bytes;
use mail_parser::MessageParser;

use crate::extract::{ExtractError, Extracted, sanitize_entry_path};
use crate::input::InputSource;

use super::message;

pub fn extract(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let bytes = source.read_bytes().map_err(ExtractError::Other)?;
    let msg = MessageParser::default()
        .parse(bytes.as_ref())
        .ok_or_else(|| ExtractError::Other(anyhow::anyhow!("could not parse email")))?;

    // Same key derivation the listing used (one shared helper), so a row
    // round-trips to its part. Keys and parts share `attachments()` order.
    let keys = message::attachment_keys(&msg);
    let parts: Vec<_> = msg.attachments().collect();
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
            bytes: Bytes::copy_from_slice(parts[idx].contents()),
            name: key.to_string(),
        },
    })
}
