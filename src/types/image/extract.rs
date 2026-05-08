//! Extract a single frame out of an animated image (GIF / WebP) as a
//! standalone PNG-encoded [`InputSource`].
//!
//! Static images (no frames, or only one) return `Unsupported` — the
//! file itself is already standalone, so there's nothing to extract.
//! The caller should peek the source directly.
//!
//! SVG keyframe animations are not yet supported here; that path lives
//! in `svg_anim` and needs a per-frame rasterizer hookup. Phase 2.

use std::io::Cursor;

use bytes::Bytes;
use image::{ImageEncoder, ImageFormat, codecs::png::PngEncoder};

use crate::extract::{ExtractError, Extracted};
use crate::input::InputSource;
use crate::types::image::pipeline::animate::{AnimFrame, decode_anim_frames};

/// Extract frame `key` (1-based) from `source`. Returns a Memory-backed
/// `InputSource` carrying PNG-encoded RGBA bytes — feed it back through
/// peek to view, or write it via `extract::write`.
pub fn extract(
    source: &InputSource,
    key: &str,
    magic_mime: Option<&str>,
) -> Result<Extracted, ExtractError> {
    let frames = decode_anim_frames(source, magic_mime)
        .map_err(ExtractError::Other)?
        .ok_or(ExtractError::Unsupported(
            "image is not animated; the file itself is already a single image",
        ))?;

    let total = frames.len();
    let frame = parse_frame_key(key, total)?;
    let encoded = encode_frame_png(&frames[frame.idx])?;
    let suggested_name = suggest_frame_name(source, frame.one_based, total);
    Ok(Extracted {
        source: InputSource::memory(encoded, suggested_name.clone()),
        suggested_name,
    })
}

struct FrameKey {
    idx: usize,
    one_based: usize,
}

fn parse_frame_key(key: &str, total: usize) -> Result<FrameKey, ExtractError> {
    let one_based: usize = key
        .parse()
        .map_err(|_| ExtractError::InvalidKey(key.to_string()))?;
    if one_based == 0 || one_based > total {
        return Err(ExtractError::NotFound(format!(
            "frame {one_based} out of range 1..={total}"
        )));
    }
    Ok(FrameKey {
        idx: one_based - 1,
        one_based,
    })
}

fn encode_frame_png(frame: &AnimFrame) -> Result<Bytes, ExtractError> {
    let rgba = frame.image.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut buf = Vec::with_capacity((w as usize) * (h as usize) * 4);
    let _ = ImageFormat::Png; // ensure image::ImageFormat is in scope for clarity
    PngEncoder::new(Cursor::new(&mut buf))
        .write_image(rgba.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| ExtractError::Other(anyhow::Error::from(e)))?;
    Ok(Bytes::from(buf))
}

fn suggest_frame_name(source: &InputSource, frame_one_based: usize, total: usize) -> String {
    let stem = source_stem(source);
    let width = total.to_string().len();
    format!("{stem}-frame-{frame_one_based:0width$}.png")
}

fn source_stem(source: &InputSource) -> String {
    if let Some(path) = source.path()
        && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
    {
        return stem.to_string();
    }
    let name = source.name();
    let trimmed = name.trim_matches(|c: char| matches!(c, '<' | '>'));
    if trimmed.is_empty() {
        "frame".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputSource;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("test-images");
        p.push(name);
        InputSource::File(p)
    }

    #[test]
    fn extract_first_gif_frame_is_valid_png() {
        let src = fixture("lightning.gif");
        let extracted = extract(&src, "1", Some("image/gif")).unwrap();
        assert!(
            extracted.suggested_name.ends_with(".png"),
            "got {}",
            extracted.suggested_name
        );
        let bytes = extracted.source.read_bytes().unwrap();
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "expected PNG header, got {:?}",
            &bytes[..bytes.len().min(8)]
        );
        // Round-trip decode to confirm the encoded PNG is well-formed.
        let _ = image::load_from_memory(&bytes).expect("PNG should decode");
    }

    #[test]
    fn extract_invalid_frame_index_errors() {
        let src = fixture("lightning.gif");
        let err = extract(&src, "0", Some("image/gif")).unwrap_err();
        assert!(matches!(err, ExtractError::NotFound(_)));

        let err = extract(&src, "9999", Some("image/gif")).unwrap_err();
        assert!(matches!(err, ExtractError::NotFound(_)));
    }

    #[test]
    fn extract_non_numeric_key_errors() {
        let src = fixture("lightning.gif");
        let err = extract(&src, "first", Some("image/gif")).unwrap_err();
        assert!(matches!(err, ExtractError::InvalidKey(_)));
    }

    #[test]
    fn extract_static_image_unsupported() {
        let src = fixture("fire.png");
        let err = extract(&src, "1", Some("image/png")).unwrap_err();
        assert!(matches!(err, ExtractError::Unsupported(_)));
    }
}
