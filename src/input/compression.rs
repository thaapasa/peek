//! Bare single-stream codec helpers. Used by `compose_modes` to
//! transparently decompress `.gz` / `.bz2` / `.xz` / `.zst` / `.lz4`
//! files into their inner content, and (re-exported via
//! `archive::extract::decompress_tar`) by tar extraction so the codec
//! dispatch lives in one place.
//!
//! Decompression is batch (reads the whole stream into a buffer).
//! Streaming inner-content rendering would need a different shape
//! since most viewers — pretty-print, syntax highlight, image decode —
//! want the full buffer upfront. The output is capped at
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

use crate::input::InputSource;
use crate::input::detect::{
    CompressionFormat, DecompressionContext, Detected, FileType, detect as redetect,
};

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
            // lzma-rs has no streaming Read wrapper, so the whole
            // plaintext lands in `out` in one shot. Size check below
            // catches over-cap; pre-allocating is pointless without a
            // streaming reader to bail mid-decode.
            let mut input = std::io::BufReader::new(raw);
            lzma_rs::xz_decompress(&mut input, &mut out)
                .map_err(|e| anyhow::anyhow!("xz decode failed: {e:?}"))?;
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
/// `compose_modes` so re-detection on the inner content routes by
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

/// Transparent decompression entry point. Called at every boundary
/// where a fresh `(source, Detected)` pair is about to drive view
/// composition — `main::run_view`, `ViewerState::push_extracted`, and
/// the retry path. For a bare single-stream wrapper this swaps both
/// values for the inner content (in-memory `InputSource` carrying the
/// decompressed bytes + a fresh `Detected` produced by re-running
/// magic / name detection on those bytes). The new `Detected` carries
/// the codec metadata in `decompressed_from` so the info view can
/// render a Compression row.
///
/// Non-Compressed sources pass through unchanged.
///
/// On decompression failure the original (compressed) source survives
/// and `decompressed_from.error` is populated; downstream
/// `compose_modes` sees `FileType::Compressed` and falls through to
/// the Hex + Info universal tail, so the user gets a raw-byte view
/// plus a Warnings row explaining the failure.
pub fn resolve_transparent(source: InputSource, detected: Detected) -> (InputSource, Detected) {
    let FileType::Compressed(fmt) = detected.file_type else {
        return (source, detected);
    };

    let outer_name = source.name().to_string();
    let raw = match source.read_bytes() {
        Ok(buf) => buf,
        Err(e) => {
            let detected = with_error(detected, fmt, 0, outer_name, format!("read failed: {e:#}"));
            return (source, detected);
        }
    };
    let compressed_size = raw.len() as u64;

    match decompress_bytes(&raw, fmt) {
        Ok(decoded) => {
            let inner_name = stripped_name(&outer_name, fmt);
            let inner = InputSource::memory(decoded, inner_name);
            let mut inner_detected = redetect(&inner)
                .unwrap_or_else(|_| Detected::new(FileType::Binary, detected.magic_mime.clone()));
            inner_detected.decompressed_from = Some(DecompressionContext {
                codec: fmt,
                compressed_size,
                outer_name,
                error: None,
            });
            (inner, inner_detected)
        }
        Err(e) => {
            let detected = with_error(detected, fmt, compressed_size, outer_name, format!("{e:#}"));
            (source, detected)
        }
    }
}

fn with_error(
    mut detected: Detected,
    codec: CompressionFormat,
    compressed_size: u64,
    outer_name: String,
    error: String,
) -> Detected {
    detected.decompressed_from = Some(DecompressionContext {
        codec,
        compressed_size,
        outer_name,
        error: Some(error),
    });
    detected
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // `single.gz` fixture wraps the ASCII payload below.
        let raw = std::fs::read(format!(
            "{}/test-data/single.gz",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let out = decompress_bytes(&raw, CompressionFormat::Gz).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn decompress_bz2_round_trip() {
        let raw = std::fs::read(format!(
            "{}/test-data/single.bz2",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let out = decompress_bytes(&raw, CompressionFormat::Bz2).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn decompress_xz_round_trip() {
        let raw = std::fs::read(format!(
            "{}/test-data/single.xz",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let out = decompress_bytes(&raw, CompressionFormat::Xz).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn decompress_zst_round_trip() {
        let raw = std::fs::read(format!(
            "{}/test-data/single.zst",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let out = decompress_bytes(&raw, CompressionFormat::Zst).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn decompress_lz4_round_trip() {
        let raw = std::fs::read(format!(
            "{}/test-data/single.lz4",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let out = decompress_bytes(&raw, CompressionFormat::Lz4).unwrap();
        assert_eq!(out.as_ref(), b"hello peek single-stream test\n");
    }

    #[test]
    fn resolve_transparent_replaces_compressed_with_inner() {
        use crate::input::InputSource;
        use crate::input::detect::detect;

        let path = format!("{}/test-data/single.gz", env!("CARGO_MANIFEST_DIR"));
        let src = InputSource::File(std::path::PathBuf::from(&path));
        let det = detect(&src).unwrap();
        // Bare `.gz` classifies as Compressed before resolve.
        assert!(matches!(
            det.file_type,
            FileType::Compressed(CompressionFormat::Gz)
        ));

        let (resolved_src, resolved_det) = resolve_transparent(src, det);
        // After resolve the inner is plain text (no extension on the
        // memory source's name, so it falls back to SourceCode).
        assert!(matches!(
            resolved_det.file_type,
            FileType::SourceCode { .. }
        ));
        let ctx = resolved_det
            .decompressed_from
            .as_ref()
            .expect("decompressed_from set");
        assert_eq!(ctx.codec, CompressionFormat::Gz);
        assert!(ctx.error.is_none());
        assert!(ctx.outer_name.ends_with("single.gz"));
        // Memory source carries the inner stripped name.
        assert!(resolved_src.name().ends_with("single"));
    }

    #[test]
    fn resolve_transparent_surfaces_decompression_error() {
        use crate::input::InputSource;
        use crate::input::detect::{Detected, FileType};

        // Plain bytes labelled as `.gz` — decode fails on the magic.
        let src = InputSource::memory(bytes::Bytes::from(b"not a gzip stream".to_vec()), "bad.gz");
        let det = Detected {
            file_type: FileType::Compressed(CompressionFormat::Gz),
            magic_mime: None,
            decompressed_from: None,
        };
        let (resolved_src, resolved_det) = resolve_transparent(src, det);
        // On failure the outer source survives and the file_type stays
        // Compressed; the error is in decompressed_from.error.
        assert!(matches!(
            resolved_det.file_type,
            FileType::Compressed(CompressionFormat::Gz)
        ));
        let ctx = resolved_det.decompressed_from.expect("context set");
        assert!(ctx.error.is_some(), "error should be populated");
        assert_eq!(resolved_src.name(), "bad.gz");
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
