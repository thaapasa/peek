use std::io::{BufReader, Cursor};
use std::time::Duration;

use anyhow::{Context, Result};
use image::DynamicImage;

use crate::input::InputSource;

/// A single decoded animation frame with its display duration.
pub struct AnimFrame {
    pub image: DynamicImage,
    pub delay: Duration,
}

// ---------------------------------------------------------------------------
// Frame decoding
// ---------------------------------------------------------------------------

/// Animated image container format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimFormat {
    Gif,
    Webp,
}

/// Detect GIF/WebP. Prefers an already-detected MIME (set by
/// `input::detect`) so we don't re-sniff bytes or re-parse the path; falls
/// back to extension (file) or magic-byte sniff (stdin) when the caller
/// has none.
fn detect_format(source: &InputSource, magic_mime: Option<&str>) -> Option<AnimFormat> {
    if let Some(format) = magic_mime.and_then(format_from_mime) {
        return Some(format);
    }
    match source {
        InputSource::File(path) => {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            match ext.as_str() {
                "gif" => Some(AnimFormat::Gif),
                "webp" => Some(AnimFormat::Webp),
                _ => None,
            }
        }
        _ => {
            let buf = source.read_bytes().ok()?;
            sniff_anim_format(&buf)
        }
    }
}

fn format_from_mime(mime: &str) -> Option<AnimFormat> {
    match mime {
        "image/gif" => Some(AnimFormat::Gif),
        "image/webp" => Some(AnimFormat::Webp),
        _ => None,
    }
}

fn sniff_anim_format(data: &[u8]) -> Option<AnimFormat> {
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some(AnimFormat::Gif)
    } else if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some(AnimFormat::Webp)
    } else {
        None
    }
}

/// Total decoded-frame budget for one animation. Every frame is held as
/// an RGBA `DynamicImage` for playback, so the whole sequence lives in
/// memory at once; this bounds that sum. 1 GiB comfortably covers real
/// animations (a 1080p 60-frame clip is ~0.5 GiB) while rejecting bombs.
const ANIM_TOTAL_BYTES_BUDGET: u64 = 1024 * 1024 * 1024;

/// Collect `Frame` results from an AnimationDecoder into `AnimFrame`s.
///
/// Two layered caps, because these decoders are built directly (not via
/// `ImageReader`) and so start out with *no* limit:
///
/// 1. **Per-frame** — `set_limits` to the `image` default (512 MiB).
///    GIF honours it (`reserve_buffer` per frame), so a huge-canvas bomb
///    (e.g. 2 frames at 65535², a 17 GB first allocation) surfaces as a
///    clean decode error instead of an OOM. WebP's decoder ignores
///    `Limits`, so for it this is a no-op (ceiling stays the 16384²/frame
///    format maximum).
///
/// 2. **Cumulative** — every decoded frame is accumulated into the
///    returned `Vec` for playback, which the per-frame cap alone does not
///    bound. A many-frame bomb (thousands of mid-size frames, or WebP
///    frames near its format ceiling) would still grow the vec without
///    limit, so the running total is checked against
///    [`ANIM_TOTAL_BYTES_BUDGET`]. This is what closes the WebP case and
///    the many-small-frames GIF case.
///
/// Either cap tripping returns `Err`; compose then falls back to the
/// static path / Hex rather than crashing.
fn collect_frames<'a, D>(mut decoder: D) -> Result<Vec<AnimFrame>>
where
    D: image::AnimationDecoder<'a> + image::ImageDecoder,
{
    decoder
        .set_limits(image::Limits::default())
        .context("failed to set decoder limits")?;
    let mut frames = Vec::new();
    let mut total_bytes: u64 = 0;
    for frame_result in decoder.into_frames() {
        let frame = frame_result.context("failed to decode frame")?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let ms = numer.checked_div(denom).unwrap_or(100);
        let delay = Duration::from_millis(ms.max(20) as u64);
        let image = DynamicImage::ImageRgba8(frame.into_buffer());
        total_bytes =
            total_bytes.saturating_add(u64::from(image.width()) * u64::from(image.height()) * 4);
        anyhow::ensure!(
            total_bytes <= ANIM_TOTAL_BYTES_BUDGET,
            "animation exceeds the {ANIM_TOTAL_BYTES_BUDGET}-byte decode budget"
        );
        frames.push(AnimFrame { image, delay });
    }
    Ok(frames)
}

/// Decode all frames from an animated image (GIF or WebP).
/// Returns `None` if the source is not an animated format or has ≤1 frame.
///
/// `magic_mime` is an upstream-detected MIME (e.g. `"image/gif"`); when
/// present it short-circuits format detection.
pub fn decode_anim_frames(
    source: &InputSource,
    magic_mime: Option<&str>,
) -> Result<Option<Vec<AnimFrame>>> {
    let Some(format) = detect_format(source, magic_mime) else {
        return Ok(None);
    };
    let frames = match (source, format) {
        (InputSource::File(path), AnimFormat::Gif) => {
            let reader = BufReader::new(std::fs::File::open(path).context("failed to open GIF")?);
            collect_frames(
                image::codecs::gif::GifDecoder::new(reader).context("failed to decode GIF")?,
            )?
        }
        (InputSource::File(path), AnimFormat::Webp) => {
            let reader = BufReader::new(std::fs::File::open(path).context("failed to open WebP")?);
            collect_frames(
                image::codecs::webp::WebPDecoder::new(reader).context("failed to decode WebP")?,
            )?
        }
        (other, AnimFormat::Gif) => {
            let buf = other
                .read_bytes()
                .context("failed to read animated GIF source")?;
            collect_frames(
                image::codecs::gif::GifDecoder::new(Cursor::new(buf))
                    .context("failed to decode GIF")?,
            )?
        }
        (other, AnimFormat::Webp) => {
            let buf = other
                .read_bytes()
                .context("failed to read animated WebP source")?;
            collect_frames(
                image::codecs::webp::WebPDecoder::new(Cursor::new(buf))
                    .context("failed to decode WebP")?,
            )?
        }
    };

    if frames.len() <= 1 {
        return Ok(None);
    }
    Ok(Some(frames))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture(rel: &str) -> InputSource {
        let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).join(rel);
        assert!(path.exists(), "fixture missing: {}", path.display());
        InputSource::File(path)
    }

    /// `gif-bomb.gif` is a 64-byte GIF declaring a 65535×65535 canvas over
    /// two frames. The animation decoders are built directly (not via
    /// `ImageReader`), so without `collect_frames`' allocation cap the first
    /// frame reserves ~17 GB and OOM-kills the process. The cap must turn
    /// that into a clean decode error instead.
    #[test]
    fn gif_bomb_errors_under_the_allocation_cap() {
        let src = fixture("test-images/gif-bomb.gif");
        let err = match decode_anim_frames(&src, Some("image/gif")) {
            Err(e) => e,
            Ok(_) => panic!("oversize animation must fail, not allocate 17 GB"),
        };
        assert!(
            err.chain().any(|c| c.to_string().contains("limit")),
            "expected an allocation-limit error, got: {err:#}"
        );
    }

    /// A normal animated GIF still decodes every frame through the same cap.
    #[test]
    fn small_animated_gif_decodes_all_frames() {
        let src = fixture("test-images/rickroll.gif");
        let frames = decode_anim_frames(&src, Some("image/gif"))
            .expect("decode ok")
            .expect("rickroll is animated");
        assert!(frames.len() > 1, "expected multiple frames");
    }
}
