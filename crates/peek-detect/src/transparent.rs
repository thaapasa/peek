//! Transparent single-stream decompression.
//!
//! Sits above the bare codecs in [`peek_io::compression`] and the
//! detection orchestrator ([`crate::detect`]): given a `(source,
//! Detected)` pair tagged [`FileType::Compressed`], it decompresses the
//! outer stream in memory and re-runs detection on the inner bytes, so
//! the user sees the inner content rendered as its real type. This is why
//! it lives in `peek-detect` rather than `peek-io` — it calls back into
//! `detect`.

use peek_io::InputSource;
use peek_io::compression::{CompressionFormat, decompress_to_source, stripped_name};

use crate::detect::{DecompressionContext, Detected, FileType, detect as redetect};

/// Transparent decompression entry point. Called at every boundary
/// where a fresh `(source, Detected)` pair is about to drive view
/// composition — `main::run_view`, `ViewerState::push_extracted`, and
/// the retry path. For a bare single-stream wrapper this swaps both
/// values for the inner content (a fresh `InputSource` carrying the
/// decompressed bytes — in-memory when small, spilled to a tempfile
/// past [`peek_io::compression::DECOMPRESS_SPOOL_THRESHOLD`] so RAM stays
/// bounded — plus a fresh `Detected` produced by re-running magic / name
/// detection on those bytes). The new `Detected` carries the codec
/// metadata in `decompressed_from` so the info view can render a
/// Compression row.
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
    let compressed_size = match source.byte_len() {
        Ok(n) => n,
        Err(e) => {
            let detected = with_error(detected, fmt, 0, outer_name, format!("read failed: {e:#}"));
            return (source, detected);
        }
    };

    let inner_name = stripped_name(&outer_name, fmt);
    match decompress_to_source(&source, fmt, inner_name) {
        Ok(inner) => {
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
    use crate::detect::detect;

    fn fixture_path(name: &str) -> String {
        // Fixtures live in the workspace-root `test-data/`.
        format!("{}/../../test-data/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn resolve_transparent_replaces_compressed_with_inner() {
        let src = InputSource::File(std::path::PathBuf::from(fixture_path("single.gz")));
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
}
