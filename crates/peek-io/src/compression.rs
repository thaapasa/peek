//! Bare single-stream codec helpers. Used by the transparent-decompression
//! path (`peek-detect`'s `resolve_transparent`) to turn `.gz` / `.bz2` /
//! `.xz` / `.zst` / `.lz4` / `.br` files into their inner content. (Tar
//! extraction has its own seekable, per-entry streaming decoder in
//! `backends::tar::decode_compressed` and does not go through here.)
//!
//! Two entry points, both fed by a streaming `Read` wrapper (xz
//! included):
//!
//! - [`decompress_to_source`] is what `resolve_transparent` uses. It
//!   streams the compressed input (never holding it whole in RAM) and
//!   spills the *output* to a tempfile past
//!   [`DECOMPRESS_SPOOL_THRESHOLD`], mirroring the archive-entry extract
//!   path. RAM stays bounded by the spool threshold no matter how large
//!   the inner file is, so a multi-hundred-MB `bigdb.sqlite.xz` opens the
//!   same way the identical db inside a `.tar.xz` does. Disk, not RAM, is
//!   the limit on the spilled path.
//! - [`decompress_bytes`] is the batch helper: it collects the whole
//!   output into one `Bytes` buffer, capped at [`MAX_DECOMPRESS_BYTES`]
//!   so a pathological ratio can't force a runaway allocation. Kept for
//!   callers that genuinely want the inner bytes in hand.
//!
//! When decompression fails (corrupt stream, truncated body, wrong
//! codec) the error string is plumbed into `Detected.decompressed_from`
//! by the caller; the viewer then falls back to a Hex view of the raw
//! compressed bytes with a Warning row in the info section.

use std::io::{Read, Write};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use tempfile::Builder;

use crate::InputSource;

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

/// Hard cap on the batch [`decompress_bytes`] helper, which collects the
/// whole output in RAM. Same class as the in-memory archive entry cap
/// (`extract.rs::MAX_EXTRACT_BYTES`). The streaming
/// [`decompress_to_source`] path is *not* bound by this — it spills past
/// [`DECOMPRESS_SPOOL_THRESHOLD`] so large inner files open.
pub const MAX_DECOMPRESS_BYTES: u64 = crate::limits::BULK_WALK_BYTES;

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

/// Output size at/above which [`decompress_to_source`] spools the
/// decompressed stream to a tempfile instead of holding it in RAM.
/// Mirrors the archive-extract spool threshold (`extract.rs`'s
/// `SPOOL_THRESHOLD`) so bare-codec and in-archive decompression bound
/// memory identically. Below it, the result stays an in-memory `Memory`
/// source so the common small case (`config.json.gz`) avoids tempdir
/// syscalls.
pub const DECOMPRESS_SPOOL_THRESHOLD: u64 = 16 * 1024 * 1024;

/// Absolute ceiling on bytes the spill-to-tempfile path writes to disk —
/// the decompression-bomb backstop. Without it, a tiny `.gz` declaring a
/// gigantic stream fills `$TMPDIR` until the OS errors `ENOSPC`; with it,
/// the spill stops and fails cleanly. Deliberately generous (gigabytes,
/// not megabytes) so legitimate large files open — it only fires on the
/// pathological case. Always enforced, including on non-interactive paths
/// (pipe / `--print`) where no confirmation prompt can intervene. Shared
/// by `decompress_to_source` here and the archive-entry spool in
/// `types::archive::extract`.
pub const MAX_SPILL_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Build a streaming decoder for `fmt` over `reader`, for the streaming
/// [`decompress_to_source`] path. Only the bytes the caller pulls get
/// inflated.
fn decoder_for<'a>(
    reader: Box<dyn Read + 'a>,
    fmt: CompressionFormat,
) -> Result<Box<dyn Read + 'a>> {
    Ok(match fmt {
        CompressionFormat::Gz => Box::new(flate2::read::GzDecoder::new(reader)),
        CompressionFormat::Bz2 => Box::new(bzip2::read::BzDecoder::new(reader)),
        CompressionFormat::Xz => Box::new(liblzma::read::XzDecoder::new(reader)),
        CompressionFormat::Zst => {
            Box::new(zstd::stream::read::Decoder::new(reader).context("zstd decoder init failed")?)
        }
        CompressionFormat::Lz4 => Box::new(lz4_flex::frame::FrameDecoder::new(reader)),
        // 4 KiB internal buffer — the crate default for the reader wrapper.
        CompressionFormat::Br => Box::new(brotli_decompressor::Decompressor::new(reader, 4096)),
    })
}

/// Decompress `source`'s bare `fmt` stream into a fresh [`InputSource`],
/// streaming both ends: the compressed input is pulled through a
/// `ByteStream` (never read whole into RAM), and the decompressed output
/// spills to a [`tempfile::NamedTempFile`] once it passes
/// [`DECOMPRESS_SPOOL_THRESHOLD`]. Smaller results stay an in-memory
/// `Memory` source. `inner_name` is the produced source's display name
/// (typically the suffix-stripped outer name).
///
/// Unlike [`decompress_bytes`], there is no [`MAX_DECOMPRESS_BYTES`]
/// ceiling — the spill path bounds RAM by the threshold, not the output
/// size, so an arbitrarily large inner file opens. Disk capacity is the
/// limit on the spilled path (a decompression bomb fails on `ENOSPC`).
pub fn decompress_to_source(
    source: &InputSource,
    fmt: CompressionFormat,
    inner_name: impl Into<String>,
) -> Result<InputSource> {
    let inner_name = inner_name.into();
    let stream = source
        .open_stream()
        .context("failed to open compressed stream")?;
    let mut decoder = decoder_for(Box::new(stream), fmt)?;

    // Inflate up to the spool threshold into memory.
    let mut buf = Vec::new();
    (&mut decoder)
        .take(DECOMPRESS_SPOOL_THRESHOLD)
        .read_to_end(&mut buf)
        .context("decompression failed")?;

    // Probe one more byte: if the stream is exhausted the result fits in
    // RAM; otherwise it's over the threshold and spills to disk.
    let mut probe = [0u8; 1];
    let extra = decoder.read(&mut probe).context("decompression failed")?;
    if extra == 0 {
        return Ok(InputSource::memory(Bytes::from(buf), inner_name));
    }

    let mut tmp = Builder::new()
        .prefix("peek-")
        .tempfile()
        .context("failed to create tempfile for decompression spill")?;
    let file = tmp.as_file_mut();
    file.write_all(&buf)
        .context("tempfile spill write failed")?;
    file.write_all(&probe[..extra])
        .context("tempfile spill write failed")?;
    // Bound the remaining spill so a decompression bomb fails cleanly
    // instead of filling the tempdir (ENOSPC). `buf` + `probe` are already
    // on disk; copy at most the rest of the ceiling, plus one byte to
    // detect overflow.
    let written = buf.len() as u64 + extra as u64;
    let remaining = MAX_SPILL_BYTES.saturating_sub(written);
    let copied = std::io::copy(&mut (&mut decoder).take(remaining + 1), file)
        .context("tempfile spill write failed")?;
    if copied > remaining {
        anyhow::bail!(
            "decompressed stream exceeds the {MAX_SPILL_BYTES}-byte spill ceiling (decompression bomb?)"
        );
    }
    Ok(InputSource::temp_file(tmp, inner_name))
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
    use crate::limits::Budget;

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

    /// Build a gzip stream of `n` zero bytes (highly compressible, so the
    /// compressed input stays tiny while the output crosses the spool
    /// threshold).
    fn gz_zeros(n: usize) -> Vec<u8> {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&vec![0u8; n]).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn decompress_to_source_small_stays_in_memory() {
        let src = InputSource::memory(Bytes::from(fixture("single.gz")), "single.gz");
        let out = decompress_to_source(&src, CompressionFormat::Gz, "single").unwrap();
        assert!(
            matches!(out, InputSource::Memory { .. }),
            "small decompress should stay in memory"
        );
        assert_eq!(
            out.read_bytes(Budget::Unbounded("test")).unwrap().as_ref(),
            b"hello peek single-stream test\n"
        );
    }

    #[test]
    fn decompress_to_source_large_spills_to_tempfile() {
        let big = 20 * 1024 * 1024; // > DECOMPRESS_SPOOL_THRESHOLD
        let src = InputSource::memory(Bytes::from(gz_zeros(big)), "big.gz");
        let out = decompress_to_source(&src, CompressionFormat::Gz, "big").unwrap();
        assert!(
            matches!(out, InputSource::TempFile { .. }),
            "over-threshold decompress should spill to a tempfile"
        );
        assert_eq!(out.byte_len().unwrap(), big as u64);
    }

    #[test]
    fn decompress_to_source_at_threshold_stays_in_memory() {
        // Exactly the threshold fits in the in-memory head read; the
        // probe byte then reads 0, so no spill.
        let exact = DECOMPRESS_SPOOL_THRESHOLD as usize;
        let src = InputSource::memory(Bytes::from(gz_zeros(exact)), "exact.gz");
        let out = decompress_to_source(&src, CompressionFormat::Gz, "exact").unwrap();
        assert!(
            matches!(out, InputSource::Memory { .. }),
            "exactly-threshold decompress should stay in memory"
        );
        assert_eq!(
            out.read_bytes(Budget::Unbounded("test")).unwrap().len(),
            exact
        );
    }
}
