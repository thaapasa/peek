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

use crate::input::detect::CompressionFormat;
use crate::types::archive::reader::ReadSeek;
use crate::viewer::listing::{EntryMtime, FlatEntry, time_from_epoch_secs};

/// Wrap a seekable tar reader in the streaming decoder for `fmt`. Shared
/// by listing and extraction so codec dispatch lives in one place. The
/// Gz/Bz2/Zst/Lz4 decoders stream (only the bytes the caller pulls get
/// inflated); xz is the exception — `lzma-rs` has no streaming reader, so
/// the whole plaintext is buffered into a `Cursor<Vec>` up front.
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
        CompressionFormat::Xz => {
            let mut reader = reader;
            let mut compressed = Vec::new();
            reader
                .read_to_end(&mut compressed)
                .context("failed to read xz stream")?;
            let mut plain = Vec::new();
            lzma_rs::xz_decompress(&mut Cursor::new(compressed), &mut plain)
                .context("failed to decompress xz")?;
            Box::new(Cursor::new(plain))
        }
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
