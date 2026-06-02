//! Tar TOC listing. Walks the tar header chain via the `tar` crate;
//! compressed tarballs decompress on the fly through a per-codec
//! streaming `Read` adapter (gzip, bzip2, xz, zstd, lz4).
//!
//! Only headers are read — entry payloads are skipped (via tar's seek on
//! a plain archive, or read-through on a compressed stream), so listing a
//! multi-GB tarball doesn't materialise payload bodies.

use std::io::Read;

use anyhow::{Context, Result};
use tar::EntryType;

use crate::input::detect::CompressionFormat;
use crate::types::archive::reader::ReadSeek;
use crate::viewer::listing::{EntryMtime, FlatEntry, time_from_epoch_secs};

/// Wrap a seekable tar reader in the streaming decoder for `fmt`. Shared
/// by listing and extraction so codec dispatch lives in one place. Every
/// codec streams — only the bytes the caller pulls get inflated.
pub(crate) fn decode_compressed(
    reader: Box<dyn ReadSeek>,
    fmt: CompressionFormat,
) -> Result<Box<dyn Read>> {
    Ok(match fmt {
        CompressionFormat::Gz => Box::new(flate2::read::GzDecoder::new(reader)),
        CompressionFormat::Bz2 => Box::new(bzip2::read::BzDecoder::new(reader)),
        CompressionFormat::Zst => Box::new(
            zstd::stream::read::Decoder::new(reader).context("failed to init zstd decoder")?,
        ),
        CompressionFormat::Lz4 => Box::new(lz4_flex::frame::FrameDecoder::new(reader)),
        CompressionFormat::Xz => Box::new(liblzma::read::XzDecoder::new(reader)),
        CompressionFormat::Br => Box::new(brotli_decompressor::Decompressor::new(reader, 4096)),
    })
}

pub(crate) fn list_plain(reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    list_from_read(reader)
}

pub(crate) fn list_gz(reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    list_from_read(decode_compressed(reader, CompressionFormat::Gz)?)
}

pub(crate) fn list_bz2(reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    list_from_read(decode_compressed(reader, CompressionFormat::Bz2)?)
}

pub(crate) fn list_zst(reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    list_from_read(decode_compressed(reader, CompressionFormat::Zst)?)
}

pub(crate) fn list_lz4(reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    list_from_read(decode_compressed(reader, CompressionFormat::Lz4)?)
}

pub(crate) fn list_xz(reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    list_from_read(decode_compressed(reader, CompressionFormat::Xz)?)
}

pub(crate) fn list_br(reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    list_from_read(decode_compressed(reader, CompressionFormat::Br)?)
}

fn list_from_read<R: Read>(reader: R) -> Result<Vec<FlatEntry>> {
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
            .map(EntryMtime::Utc);
        let mode = header.mode().ok();
        out.push(FlatEntry {
            path: path_cow,
            size,
            mtime,
            mode,
            is_dir,
        });
    }
    Ok(out)
}
