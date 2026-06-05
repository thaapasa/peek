//! Bare single-stream codec helpers. Used by the transparent-decompression
//! path (`peek-detect`'s `resolve_transparent`) to turn `.gz` / `.bz2` /
//! `.xz` / `.zst` / `.lz4` / `.br` files into their inner content. (Tar
//! extraction has its own seekable, per-entry streaming decoder in
//! `backends::tar::decode_compressed` and does not go through here.)
//!
//! Decompression here is batch — the whole stream is read into a buffer —
//! because the transparent path feeds viewers (pretty-print, syntax
//! highlight, image decode) that want the full inner content upfront.
//! Every codec, xz included, runs through a streaming `Read` wrapper, but
//! the output is collected in one shot and capped at
//! [`MAX_DECOMPRESS_BYTES`] so a pathological compression ratio can't
//! force a runaway allocation.
//!
//! When decompression fails (corrupt stream, truncated body, wrong
//! codec) the error string is plumbed into `Detected.decompressed_from`
//! by the caller; the viewer then falls back to a Hex view of the raw
//! compressed bytes with a Warning row in the info section.

use std::io::Read;

use anyhow::{Context, Result, bail};
use bytes::Bytes;

/// Bare single-stream compression codec. Detected when the file has
/// only one of these as its outer wrapper (e.g. `notes.txt.gz`); the
/// viewer transparently decompresses and renders the inner content as
/// whatever it actually is.
///
/// Lives in `peek-io` because transparent decompression is an
/// input-source transformation (compressed bytes → in-memory inner
/// source); the codec functions below are the consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionFormat {
    /// gzip stream (`.gz`).
    Gz,
    /// bzip2 stream (`.bz2`).
    Bz2,
    /// xz / LZMA2 stream (`.xz`).
    Xz,
    /// zstd stream (`.zst`).
    Zst,
    /// lz4 frame stream (`.lz4`).
    Lz4,
    /// brotli stream (`.br`). No magic header — extension-only.
    Br,
}

impl CompressionFormat {
    /// Short codec name for the info-section Compression row.
    pub fn codec_label(self) -> &'static str {
        match self {
            Self::Gz => "gzip",
            Self::Bz2 => "bzip2",
            Self::Xz => "xz",
            Self::Zst => "zstd",
            Self::Lz4 => "lz4",
            Self::Br => "brotli",
        }
    }

    /// Lowercased filename suffix used for name-strip when building the
    /// in-memory decompressed source's name.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Gz => ".gz",
            Self::Bz2 => ".bz2",
            Self::Xz => ".xz",
            Self::Zst => ".zst",
            Self::Lz4 => ".lz4",
            Self::Br => ".br",
        }
    }
}

/// Hard cap on a transparently-decompressed bare stream. Matches the
/// archive entry cap (`extract.rs::MAX_EXTRACT_BYTES`) — a single
/// decompressed file shouldn't be allowed to balloon past the same
/// limit a single extracted archive entry has.
pub const MAX_DECOMPRESS_BYTES: u64 = 256 * 1024 * 1024;

/// Decompress `raw` according to `fmt`. Returns the inner bytes, or an
/// error explaining the codec failure / cap breach.
pub fn decompress_bytes(raw: &[u8], fmt: CompressionFormat) -> Result<Bytes> {
    // Cap reads at one byte past the limit so we can distinguish
    // "exactly at cap" from "exceeded cap".
    let take_limit = MAX_DECOMPRESS_BYTES + 1;
    let mut out: Vec<u8> = Vec::new();
    match fmt {
        CompressionFormat::Gz => {
            flate2::read::GzDecoder::new(raw)
                .take(take_limit)
                .read_to_end(&mut out)
                .context("gzip decode failed")?;
        }
        CompressionFormat::Bz2 => {
            bzip2::read::BzDecoder::new(raw)
                .take(take_limit)
                .read_to_end(&mut out)
                .context("bzip2 decode failed")?;
        }
        CompressionFormat::Xz => {
            liblzma::read::XzDecoder::new(raw)
                .take(take_limit)
                .read_to_end(&mut out)
                .context("xz decode failed")?;
        }
        CompressionFormat::Zst => {
            zstd::stream::read::Decoder::new(raw)
                .context("zstd decoder init failed")?
                .take(take_limit)
                .read_to_end(&mut out)
                .context("zstd decode failed")?;
        }
        CompressionFormat::Lz4 => {
            lz4_flex::frame::FrameDecoder::new(raw)
                .take(take_limit)
                .read_to_end(&mut out)
                .context("lz4 decode failed")?;
        }
        CompressionFormat::Br => {
            // 4 KiB internal buffer — matches the crate's own default
            // for the reader wrapper; the outer `.take` enforces the cap.
            brotli_decompressor::Decompressor::new(raw, 4096)
                .take(take_limit)
                .read_to_end(&mut out)
                .context("brotli decode failed")?;
        }
    }
    if out.len() as u64 > MAX_DECOMPRESS_BYTES {
        bail!(
            "decompressed stream exceeds {MAX_DECOMPRESS_BYTES}-byte cap (got > {MAX_DECOMPRESS_BYTES} bytes)"
        );
    }
    Ok(Bytes::from(out))
}

/// Best-effort name for the in-memory decompressed source. Strips the
/// codec's filename suffix when present; falls back to a sentinel for
/// stdin or a `-decompressed` suffix for nameless files. Used by
/// `resolve_transparent` so re-detection on the inner content routes by
/// extension where possible.
pub fn stripped_name(source_name: &str, fmt: CompressionFormat) -> String {
    let suffix = fmt.suffix();
    let lower = source_name.to_ascii_lowercase();
    if lower.ends_with(suffix) {
        return source_name[..source_name.len() - suffix.len()].to_string();
    }
    if source_name == "<stdin>" {
        return "decompressed".to_string();
    }
    format!("{source_name}-decompressed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        // Fixtures live in the workspace-root `test-data/`; this crate's
        // manifest dir is `crates/peek-io`, so hop up two levels.
        std::fs::read(format!(
            "{}/../../test-data/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn strips_compression_suffix() {
        assert_eq!(
            stripped_name("notes.txt.gz", CompressionFormat::Gz),
            "notes.txt"
        );
        assert_eq!(
            stripped_name("backup.tar.bz2", CompressionFormat::Bz2),
            "backup.tar"
        );
        assert_eq!(stripped_name("LOG.XZ", CompressionFormat::Xz), "LOG");
        assert_eq!(
            stripped_name("payload.lz4", CompressionFormat::Lz4),
            "payload"
        );
    }

    #[test]
    fn fallback_for_no_extension() {
        assert_eq!(
            stripped_name("anonymous", CompressionFormat::Gz),
            "anonymous-decompressed"
        );
    }

    #[test]
    fn stdin_name_collapses_to_decompressed() {
        assert_eq!(
            stripped_name("<stdin>", CompressionFormat::Gz),
            "decompressed"
        );
    }

    #[test]
    fn decompress_gz_round_trip() {
        let out = decompress_bytes(&fixture("single.gz"), CompressionFormat::Gz).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn decompress_bz2_round_trip() {
        let out = decompress_bytes(&fixture("single.bz2"), CompressionFormat::Bz2).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn decompress_xz_round_trip() {
        let out = decompress_bytes(&fixture("single.xz"), CompressionFormat::Xz).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn decompress_zst_round_trip() {
        let out = decompress_bytes(&fixture("single.zst"), CompressionFormat::Zst).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn decompress_lz4_round_trip() {
        let out = decompress_bytes(&fixture("single.lz4"), CompressionFormat::Lz4).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn decompress_br_round_trip() {
        let out = decompress_bytes(&fixture("single.br"), CompressionFormat::Br).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn decompress_corrupt_errors_cleanly() {
        // gzip magic followed by garbage — header parses, body decode
        // bombs.
        let mut bad = vec![0x1f, 0x8b, 0x08, 0x00];
        bad.extend_from_slice(&[0u8; 16]);
        bad.extend_from_slice(b"\xff\xff\xff\xff\xff\xff");
        assert!(decompress_bytes(&bad, CompressionFormat::Gz).is_err());
    }
}
