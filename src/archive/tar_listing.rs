//! Tar TOC listing. Walks the tar header chain via the `tar` crate;
//! compressed tarballs decompress on the fly through a per-codec
//! streaming `Read` adapter (gzip, bzip2, zstd) or a one-shot batch
//! decompress for codecs lacking a streaming wrapper (xz via `lzma-rs`).
//!
//! Only headers are read for streaming codecs — entry payloads are
//! skipped via tar's seek, so listing a multi-GB tarball doesn't
//! decompress payload bodies. The xz path is the exception: `lzma-rs`
//! exposes only batch decompression, so the full plaintext is buffered
//! before tar parses it. Acceptable for typical archive sizes; can be
//! optimized later by switching to a streaming xz crate.

use std::io::{Cursor, Read};

use anyhow::{Context, Result};
use tar::EntryType;

use super::{ArchiveEntry, ArchiveMtime, ReadSeek, time_from_epoch_secs};

pub(super) fn list_plain(reader: Box<dyn ReadSeek>) -> Result<Vec<ArchiveEntry>> {
    list_from_read(reader)
}

pub(super) fn list_gz(reader: Box<dyn ReadSeek>) -> Result<Vec<ArchiveEntry>> {
    let dec = flate2::read::GzDecoder::new(reader);
    list_from_read(dec)
}

pub(super) fn list_bz2(reader: Box<dyn ReadSeek>) -> Result<Vec<ArchiveEntry>> {
    let dec = bzip2_rs::DecoderReader::new(reader);
    list_from_read(dec)
}

pub(super) fn list_zst(reader: Box<dyn ReadSeek>) -> Result<Vec<ArchiveEntry>> {
    let dec = zstd::stream::read::Decoder::new(reader).context("failed to init zstd decoder")?;
    list_from_read(dec)
}

pub(super) fn list_xz(mut reader: Box<dyn ReadSeek>) -> Result<Vec<ArchiveEntry>> {
    let mut compressed = Vec::new();
    reader
        .read_to_end(&mut compressed)
        .context("failed to read xz stream")?;
    let mut plain = Vec::new();
    lzma_rs::xz_decompress(&mut Cursor::new(compressed), &mut plain)
        .context("failed to decompress xz")?;
    list_from_read(Cursor::new(plain))
}

fn list_from_read<R: Read>(reader: R) -> Result<Vec<ArchiveEntry>> {
    let mut archive = tar::Archive::new(reader);
    let mut out = Vec::new();
    for entry in archive.entries().context("failed to read tar archive")? {
        let entry = entry.context("failed to read tar entry")?;
        let header = entry.header();
        let path_cow = entry
            .path()
            .context("failed to decode tar entry path")?
            .to_string_lossy()
            .into_owned();
        let entry_type = header.entry_type();
        let is_dir = entry_type == EntryType::Directory || path_cow.ends_with('/');
        let size = header.size().unwrap_or(0);
        let mtime = header
            .mtime()
            .ok()
            .and_then(time_from_epoch_secs)
            .map(ArchiveMtime::Utc);
        let mode = header.mode().ok();
        out.push(ArchiveEntry {
            path: normalize(&path_cow),
            size,
            mtime,
            mode,
            is_dir,
        });
    }
    Ok(out)
}

/// Strip a redundant `./` prefix so paths line up with the zip backend.
/// Bare `.` / `./` (the archive root) is preserved as `./` — stripping it
/// would render as `/` and look like a Unix root path.
fn normalize(p: &str) -> String {
    match p {
        "." | "./" => "./".to_string(),
        _ => p.strip_prefix("./").unwrap_or(p).to_string(),
    }
}
