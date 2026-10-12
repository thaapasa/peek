//! Extract one LMDB value by its row inner path ([`super::cells`]):
//! `<db>/<key>[/<dup>]`. The key is looked up by B+tree descent, so the
//! extract stays correct even if the file changed since the table loaded.
//!
//! An overflow value is contiguous on disk, so it comes back as a
//! `FileRange` over the database file — no copy, any size. Inline values
//! (under half a page) come back in memory. Either way peek re-detects
//! the bytes: a JSON value opens as JSON, a PNG as an image.

use anyhow::anyhow;
use peek_io::InputSource;

use super::cells::{self, RowPath};
use super::env::LmdbEnv;
use super::format::{DbRecord, F_DUPDATA, F_SUBDATA, V10};
use super::tree::{self, ValueRef};
use crate::extract::{ExtractError, Extracted};

pub fn extract(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let path =
        cells::parse_row_path(key).ok_or_else(|| ExtractError::InvalidKey(key.to_string()))?;
    let env = LmdbEnv::open(source).map_err(ExtractError::Other)?;
    let db = resolve_db(&env, &path, key)?;
    let not_found = || ExtractError::NotFound(key.to_string());
    let node = tree::find(&env, &db, &path.key)
        .map_err(ExtractError::Other)?
        .ok_or_else(not_found)?;
    let value = tree::node_value(&env, &db, &node, path.dup.unwrap_or(0))
        .map_err(ExtractError::Other)?
        .ok_or_else(not_found)?;

    let suggested_name = cells::suggested_name(&path.key);
    let source = match value {
        ValueRef::Inline(bytes) => InputSource::memory(bytes, suggested_name.clone()),
        ValueRef::Overflow { pgno, len } => {
            let off = env
                .overflow_offset(pgno, len)
                .map_err(ExtractError::Other)?;
            source.subrange(off, len, suggested_name.clone())
        }
        ValueRef::Database(_) => {
            return Err(ExtractError::Unsupported(
                "row is a database record — open it from the database list",
            ));
        }
    };
    Ok(Extracted {
        suggested_name,
        source,
    })
}

fn resolve_db(env: &LmdbEnv, path: &RowPath, key: &str) -> Result<DbRecord, ExtractError> {
    let Some(name) = &path.db else {
        return Ok(env.meta.main);
    };
    // 1.0 stores names NUL-terminated; 0.9 doesn't. Try the native spelling
    // first: both descents run before any full scan of MAIN.
    let mut nul_name = name.to_vec();
    nul_name.push(0);
    let names: [&[u8]; 2] = if env.variant == V10 {
        [&nul_name, name]
    } else {
        [name, &nul_name]
    };
    let node = tree::find_any(env, &env.meta.main, &names)
        .map_err(ExtractError::Other)?
        .ok_or_else(|| ExtractError::NotFound(key.to_string()))?;
    if node.flags & F_SUBDATA == 0 || node.flags & F_DUPDATA != 0 {
        return Err(ExtractError::NotFound(key.to_string()));
    }
    let data = node.data().map_err(ExtractError::Other)?;
    DbRecord::parse(&data).ok_or_else(|| ExtractError::Other(anyhow!("bad DB record for {key}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::lmdb::testing::{catalogs, event_log};

    fn bytes(e: &Extracted) -> Vec<u8> {
        e.source
            .read_bytes(peek_io::limits::Budget::Unbounded("test"))
            .unwrap()
            .to_vec()
    }

    #[test]
    fn extracts_inline_text_value() {
        for source in catalogs() {
            let e = extract(&source.clone(), "users/user:00042").unwrap();
            assert!(bytes(&e).starts_with(b"{\"id\":42,"));
            assert_eq!(e.suggested_name, "user_00042");
        }
    }

    #[test]
    fn overflow_value_is_a_file_range() {
        for source in catalogs() {
            let source = source.clone();
            // Find the 40 KB blob's key through the blobs DB rows.
            let e = extract(&source, "blobs/x:00000003").unwrap();
            assert!(matches!(e.source, InputSource::FileRange { .. }));
            assert_eq!(bytes(&e).len(), 40_000);
        }
    }

    #[test]
    fn dup_index_selects_the_value() {
        for source in catalogs() {
            let first = bytes(&extract(&source.clone(), "tags/beta/0").unwrap());
            let last = bytes(&extract(&source.clone(), "tags/beta/5").unwrap());
            assert!(first.starts_with(b"user:") && last.starts_with(b"user:"));
            assert!(first < last, "dups come back in sorted order");
            assert!(matches!(
                extract(&source.clone(), "tags/beta/6"),
                Err(ExtractError::NotFound(_))
            ));
        }
    }

    #[test]
    fn main_and_integer_keys() {
        for source in catalogs() {
            let e = extract(&source.clone(), "@main/schema_version").unwrap();
            assert_eq!(bytes(&e), b"3");
            let path = cells::row_path("counters", &256u32.to_le_bytes(), None);
            assert_eq!(
                bytes(&extract(&source.clone(), &path).unwrap()),
                b"count 256"
            );
            let key = (1_760_000_000u64).to_le_bytes();
            let path = cells::row_path("@main", &key, None);
            assert_eq!(bytes(&extract(&event_log(), &path).unwrap()), b"boot");
        }
    }

    #[test]
    fn bad_keys_are_rejected() {
        for source in catalogs() {
            assert!(matches!(
                extract(&source.clone(), "users"),
                Err(ExtractError::InvalidKey(_))
            ));
            assert!(matches!(
                extract(&source.clone(), "nosuchdb/k"),
                Err(ExtractError::NotFound(_))
            ));
            // A named-DB record in MAIN (NUL-terminated name under 1.0).
            let unsupported = ["@main/users", &cells::row_path("@main", b"users\0", None)]
                .iter()
                .filter(|p| matches!(extract(&source, p), Err(ExtractError::Unsupported(_))))
                .count();
            assert_eq!(unsupported, 1);
        }
    }
}
