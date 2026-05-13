//! ar(1) archive TOC reader. Used for `.deb` packages — Debian
//! binary packages are an ar archive with three members:
//! `debian-binary` (text), `control.tar.{gz|xz|zst}`, and
//! `data.tar.{gz|xz|zst}`. Recursive peek over the data tarball walks
//! the package's installed files via the existing tar backend.
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
//! Extended naming: GNU-style long names live in a synthetic
//! `//` member or are prefixed with `#1/<len>`. `.deb` uses short
//! names exclusively, so we only handle the short-name path here
//! and lossily display the rest.
//!
//! No payload extraction at the listing layer — the shared archive
//! `extract` impl reads bytes via the same offset map.

use std::io::Read;

use anyhow::{Context, Result, bail};

use crate::types::archive::reader::ReadSeek;
use crate::viewer::listing::{EntryMtime, FlatEntry, time_from_epoch_secs};

const HEADER_LEN: usize = 60;
const GLOBAL_MAGIC: &[u8; 8] = b"!<arch>\n";
const ENTRY_TRAILER: &[u8; 2] = b"`\n";

pub(crate) fn list(mut reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    let mut magic = [0u8; 8];
    reader
        .read_exact(&mut magic)
        .context("ar: failed to read global magic")?;
    if &magic != GLOBAL_MAGIC {
        bail!("not an ar archive: missing !<arch> magic");
    }

    let mut out = Vec::new();
    let mut header = [0u8; HEADER_LEN];
    loop {
        match reader.read(&mut header)? {
            0 => break,
            n if n < HEADER_LEN => {
                // Trailing pad byte; some archives end on an odd byte
                // followed by a `\n` and nothing else. Bail cleanly.
                break;
            }
            _ => {}
        }
        if &header[58..60] != ENTRY_TRAILER {
            if header[0] == b'\n' {
                continue;
            }
            bail!("ar: malformed entry header (missing trailer)");
        }
        let raw_name = decode_name(&header[..16]);
        let mtime_secs = decode_decimal(&header[16..28]);
        let mode = decode_octal(&header[40..48]);
        let total_size: u64 = decode_decimal(&header[48..58]).unwrap_or(0).max(0) as u64;

        // BSD `ar` (used by macOS) encodes names ≥ 16 chars OR
        // containing spaces as `#1/<len>`, with the actual name
        // prefixed onto the payload. Strip that prefix so the listing
        // shows the real filename.
        let (name, payload_size) = if let Some(rest) = raw_name.strip_prefix("#1/") {
            let name_len: u64 = rest.trim().parse().unwrap_or(0);
            if name_len > total_size {
                ("?".to_string(), total_size)
            } else {
                let mut nbuf = vec![0u8; name_len as usize];
                reader
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

        let visible = !matches!(
            name.as_str(),
            "//" | "/" | "/SYM64/" | "__.SYMDEF SORTED" | "__.SYMDEF"
        );
        if visible {
            out.push(FlatEntry {
                path: name,
                size: payload_size,
                mtime: mtime_secs
                    .and_then(|s| time_from_epoch_secs(s as u64))
                    .map(EntryMtime::Utc),
                mode: mode.map(|m| m as u32),
                is_dir: false,
            });
        }

        let pad = total_size % 2;
        // total_size already accounts for the BSD name prefix, so skip
        // exactly the payload bytes left after the name read above.
        skip_bytes(&mut reader, payload_size + pad)?;
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

fn skip_bytes(reader: &mut Box<dyn ReadSeek>, mut count: u64) -> Result<()> {
    let mut buf = [0u8; 4096];
    while count > 0 {
        let want = (count as usize).min(buf.len());
        let n = reader.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        count -= n as u64;
    }
    Ok(())
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
}
