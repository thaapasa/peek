use std::io::{BufReader, Cursor};
use std::time::Duration;

use anyhow::{Context, Result};
use image::DynamicImage;

use crate::input::InputSource;

use super::ImageConfig;
use super::render;

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
        InputSource::Stdin { data } => sniff_anim_format(data),
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

/// Collect `Frame` results from an AnimationDecoder into `AnimFrame`s.
fn collect_frames<'a, D: image::AnimationDecoder<'a>>(decoder: D) -> Result<Vec<AnimFrame>> {
    let mut frames = Vec::new();
    for frame_result in decoder.into_frames() {
        let frame = frame_result.context("failed to decode frame")?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let ms = numer.checked_div(denom).unwrap_or(100);
        let delay = Duration::from_millis(ms.max(20) as u64);
        let image = DynamicImage::ImageRgba8(frame.into_buffer());
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
        (InputSource::Stdin { data }, AnimFormat::Gif) => {
            let reader = Cursor::new(data.clone());
            collect_frames(
                image::codecs::gif::GifDecoder::new(reader).context("failed to decode GIF")?,
            )?
        }
        (InputSource::Stdin { data }, AnimFormat::Webp) => {
            let reader = Cursor::new(data.clone());
            collect_frames(
                image::codecs::webp::WebPDecoder::new(reader).context("failed to decode WebP")?,
            )?
        }
    };

    if frames.len() <= 1 {
        return Ok(None);
    }
    Ok(Some(frames))
}

// ---------------------------------------------------------------------------
// Frame rendering (shared with `modes::AnimationMode`)
// ---------------------------------------------------------------------------

pub(crate) fn render_frame(frame: &AnimFrame, config: &ImageConfig) -> Vec<String> {
    let mut term = render::TermSize::detect();
    term.rows = term.rows.saturating_sub(1);
    render::render_decoded(frame.image.clone(), config, term)
}
