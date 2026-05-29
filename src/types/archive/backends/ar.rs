//! ar(1) archive reader. Used for `.deb` packages — Debian binary
//! packages are an ar archive with three members: `debian-binary`
//! (text), `control.tar.{gz|xz|zst}`, and `data.tar.{gz|xz|zst}`.
//! Recursive peek over the data tarball walks the package's installed
//! files via the existing tar backend.
//!
//! Format (System V / GNU variant most `.deb` files use):
//!
//! - 8-byte global magic: `!<arch>\n`
//! - Per entry, a 60-byte ASCII header:
//!     - 16 bytes: file name (space-padded, may end in `/`)
//!     - 12 bytes: mtime (decimal seconds since epoch)
//!     - 6 bytes:  uid (decimal, often 0)
//!     - 6 bytes:  gid (decimal, often 0)
//!     - 8 bytes:  mode (octal, e.g. `100644`)
//!     - 10 bytes: size in bytes (decimal)
//!     - 2 bytes:  trailer `` `\n ``
//! - Payload follows; padded to 2-byte boundary with `\n`.
//!
//! Extended naming: GNU-style long names live in a synthetic `//`
//! member or are prefixed with `#1/<len>`. `.deb` uses short names
//! exclusively; the BSD `#1/<len>` prefix is decoded, the GNU `//`
//! string table is not (its members display lossily).
//!
//! [`ArReader`] is the single header-chain parser, shared by listing
//! ([`list`]) and the entry [`extract`](crate::types::archive::extract)
//! path — same split as [`CpioReader`](super::cpio) — so the format
//! logic lives in exactly one place.

use std::io::{self, Read};

use anyhow::{Context, Result, bail};

use crate::types::archive::reader::ReadSeek;
use crate::viewer::listing::{EntryMtime, FlatEntry, time_from_epoch_secs};

const HEADER_LEN: usize = 60;
const GLOBAL_MAGIC: &[u8; 8] = b"!<arch>\n";
const ENTRY_TRAILER: &[u8; 2] = b"`\n";

/// Sanity cap on a BSD long-name length (`#1/<len>` header). Real member
/// names fit far under this; a bogus value would otherwise size a huge
/// buffer from an untrusted field.
const MAX_AR_NAME: u64 = 4096;

/// One ar member header. `size` is the payload size after any BSD
/// long-name prefix has been stripped.
pub(crate) struct ArEntry {
    pub name: String,
    pub size: u64,
    pub mtime: Option<i64>,
    pub mode: Option<u64>,
}

/// State machine over the ar header chain. Tracks the current entry's
/// unconsumed payload + padding so the caller can either skip (default
/// on the next [`Self::next_entry`]) or stream the body via
/// [`Self::body`] before advancing.
pub(crate) struct ArReader<R: Read> {
    inner: R,
    /// Body + alignment padding of the current entry still to consume
    /// before the next header can be read.
    pending: u64,
}

impl<R: Read> ArReader<R> {
    /// Open over `inner`, validating the 8-byte global magic.
    pub(crate) fn new(mut inner: R) -> Result<Self> {
        let mut magic = [0u8; 8];
        inner
            .read_exact(&mut magic)
            .context("ar: failed to read global magic")?;
        if &magic != GLOBAL_MAGIC {
            bail!("not an ar archive: missing !<arch> magic");
        }
        Ok(Self { inner, pending: 0 })
    }

    /// Advance to the next entry header, draining any unread payload +
    /// padding of the previous entry first. Returns `Ok(None)` at a clean
    /// EOF on a header boundary.
    ///
    /// Uses `read_exact`, not a single `read`: a streaming source
    /// (`RangeReadSeek` for ar-in-archive) can short-read mid-stream, and
    /// treating that as end-of-archive would silently truncate the walk.
    pub(crate) fn next_entry(&mut self) -> Result<Option<ArEntry>> {
        self.drain_pending()?;

        let mut header = [0u8; HEADER_LEN];
        match self.inner.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        if &header[58..60] != ENTRY_TRAILER {
            bail!("ar: malformed entry header (missing trailer)");
        }

        let raw_name = decode_name(&header[..16]);
        let mtime = decode_decimal(&header[16..28]);
        let mode = decode_octal(&header[40..48]);
        let total_size: u64 = decode_decimal(&header[48..58]).unwrap_or(0).max(0) as u64;

        // BSD `ar` (macOS) encodes names ≥ 16 chars or containing spaces
        // as `#1/<len>`, with the real name prefixed onto the payload.
        // Strip it so callers see the actual filename.
        let (name, size) = if let Some(rest) = raw_name.strip_prefix("#1/") {
            let name_len: u64 = rest.trim().parse().unwrap_or(0);
            // Reject an out-of-range length before allocating.
            if name_len > total_size || name_len > MAX_AR_NAME {
                ("?".to_string(), total_size)
            } else {
                let mut nbuf = vec![0u8; name_len as usize];
                self.inner
                    .read_exact(&mut nbuf)
                    .context("ar: failed to read BSD long name")?;
                let n = std::str::from_utf8(&nbuf)
                    .unwrap_or("?")
                    .trim_end_matches('\0')
                    .to_string();
                (n, total_size - name_len)
            }
        } else {
            (raw_name, total_size)
        };

        // Payload + 2-byte alignment padding still ahead. `total_size`
        // includes any BSD name prefix already read, so what's left is
        // the remaining payload (`size`) plus the pad.
        self.pending = size + total_size % 2;
        Ok(Some(ArEntry {
            name,
            size,
            mtime,
            mode,
        }))
    }

    /// Stream the current entry's body as a size-limited reader. Call
    /// right after `next_entry` returned the matching entry; the trailing
    /// padding is drained on the next `next_entry`.
    pub(crate) fn body(&mut self, size: u64) -> io::Take<&mut R> {
        // Leave only the alignment padding pending.
        self.pending = self.pending.saturating_sub(size);
        (&mut self.inner).take(size)
    }

    fn drain_pending(&mut self) -> Result<()> {
        if self.pending == 0 {
            return Ok(());
        }
        let want = self.pending;
        let n = io::copy(&mut (&mut self.inner).take(want), &mut io::sink())?;
        if n != want {
            bail!("truncated ar archive (expected {want} more bytes)");
        }
        self.pending = 0;
        Ok(())
    }
}

pub(crate) fn list(reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    let mut ar = ArReader::new(reader)?;
    let mut out = Vec::new();
    while let Some(entry) = ar.next_entry()? {
        // Synthetic GNU members (long-name table, symbol index) aren't
        // real files — hide them from the TOC.
        let hidden = matches!(
            entry.name.as_str(),
            "//" | "/" | "/SYM64/" | "__.SYMDEF SORTED" | "__.SYMDEF"
        );
        if hidden {
            continue;
        }
        out.push(FlatEntry {
            path: entry.name,
            size: entry.size,
            mtime: entry
                .mtime
                .and_then(|s| time_from_epoch_secs(s as u64))
                .map(EntryMtime::Utc),
            mode: entry.mode.map(|m| m as u32),
            is_dir: false,
        });
    }
    Ok(out)
}

fn decode_name(bytes: &[u8]) -> String {
    let raw = std::str::from_utf8(bytes).unwrap_or("");
    let trimmed = raw.trim_end_matches(' ').trim_end_matches('/');
    trimmed.to_string()
}

fn decode_decimal(bytes: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(bytes).ok()?;
    s.trim().parse().ok()
}

fn decode_octal(bytes: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(bytes).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    u64::from_str_radix(s, 8).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a minimal in-memory ar archive with a single entry.
    fn synth_ar(entry_name: &str, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(GLOBAL_MAGIC);
        let mut header = [b' '; HEADER_LEN];
        // Name (with `/` terminator at position len)
        let nm = entry_name.as_bytes();
        header[..nm.len()].copy_from_slice(nm);
        header[nm.len()] = b'/';
        // mtime
        let mt = b"0";
        header[16..16 + mt.len()].copy_from_slice(mt);
        // uid / gid: leave blank
        // mode: octal "100644"
        let mode = b"100644";
        header[40..40 + mode.len()].copy_from_slice(mode);
        // size
        let sz = format!("{}", payload.len());
        header[48..48 + sz.len()].copy_from_slice(sz.as_bytes());
        // trailer
        header[58..60].copy_from_slice(ENTRY_TRAILER);
        buf.extend_from_slice(&header);
        buf.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            buf.push(b'\n');
        }
        buf
    }

    #[test]
    fn lists_single_entry() {
        let bytes = synth_ar("debian-binary", b"2.0\n");
        let reader: Box<dyn ReadSeek> = Box::new(Cursor::new(bytes));
        let entries = list(reader).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "debian-binary");
        assert_eq!(entries[0].size, 4);
        assert!(!entries[0].is_dir);
    }

    #[test]
    fn rejects_non_ar() {
        let reader: Box<dyn ReadSeek> = Box::new(Cursor::new(b"not an ar archive!".to_vec()));
        assert!(list(reader).is_err());
    }

    /// Two entries with an odd-size payload (forces a pad byte) — the
    /// reader must drain payload + padding to land on the next header.
    #[test]
    fn walks_padded_multi_entry() {
        let mut bytes = synth_ar("first", b"odd"); // 3 bytes → 1 pad
        bytes.extend_from_slice(&synth_ar_no_magic("second", b"two\n"));
        let reader: Box<dyn ReadSeek> = Box::new(Cursor::new(bytes));
        let entries = list(reader).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, vec!["first", "second"]);
        assert_eq!(entries[0].size, 3);
        assert_eq!(entries[1].size, 4);
    }

    fn synth_ar_no_magic(entry_name: &str, payload: &[u8]) -> Vec<u8> {
        let full = synth_ar(entry_name, payload);
        full[GLOBAL_MAGIC.len()..].to_vec()
    }
}
