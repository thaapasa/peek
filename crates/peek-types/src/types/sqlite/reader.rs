//! Read-only `rusqlite::Connection` opener that copes with every
//! [`InputSource`] flavour.
//!
//! `rusqlite` opens by path, so a `File` source goes straight in.
//! `Memory` / `FileRange` / `TempFile` sources have no externally
//! callable path (the `TempFile` path is internal scratch), so the
//! reader spools the bytes into a fresh `tempfile::NamedTempFile`
//! and opens against that, holding the temp file in an `Arc` for the
//! connection's lifetime — drop it and the file unlinks.
//!
//! Connection flags: `SQLITE_OPEN_READ_ONLY`. No `SQLITE_OPEN_URI` —
//! peek opens user-supplied paths, not URI strings, and URI parsing
//! could let the path try to invoke obscure SQLite features.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use peek_io::InputSource;
use rusqlite::{Connection, OpenFlags};
use tempfile::NamedTempFile;

/// Owns a read-only SQLite connection plus the spooled temp file (if
/// any) that backs it. Drop order is connection → temp file, so the
/// file outlives every query against it.
pub struct SqliteReader {
    pub conn: Connection,
    /// Kept alive for the connection's lifetime when the source had no
    /// on-disk path. `Arc` so future read paths could share the spool
    /// between a reader and a row-cursor wrapper without duplicating
    /// the materialisation.
    _spool: Option<Arc<NamedTempFile>>,
}

impl SqliteReader {
    pub fn open(source: &InputSource) -> Result<Self> {
        let (path, spool) = resolve_path(source)?;
        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening SQLite database at {}", path.display()))?;
        Ok(Self {
            conn,
            _spool: spool,
        })
    }
}

fn resolve_path(source: &InputSource) -> Result<(PathBuf, Option<Arc<NamedTempFile>>)> {
    if let Some(p) = source.disk_path() {
        return Ok((p.to_path_buf(), None));
    }
    let bytes = source
        // Whole read into RAM before spooling — a one-pass materialization.
        .read_bytes(peek_io::limits::Budget::BulkWalk("SQLite spool"))
        .context("reading SQLite source into memory before spooling to temp file")?;
    let mut tmp = NamedTempFile::new().context("creating temp file for SQLite spool")?;
    tmp.write_all(&bytes)
        .context("writing SQLite bytes to temp spool")?;
    tmp.flush().context("flushing SQLite temp spool")?;
    let path = tmp.path().to_path_buf();
    Ok((path, Some(Arc::new(tmp))))
}
