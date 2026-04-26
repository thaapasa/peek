use std::io::{BufReader, Cursor};
use std::time::Duration;

use anyhow::{Context, Result};
use image::{AnimationDecoder, DynamicImage, GenericImageView};

use crate::input::detect::Detected;
use crate::input::InputSource;
use crate::theme::PeekThemeName;
use crate::viewer::modes::{AnimationMode, HelpMode, HexMode, InfoMode, Mode};
use crate::viewer::ui::{Action, GLOBAL_ACTIONS};

use super::render;
use super::ImageConfig;

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

/// Detect GIF/WebP from either the file extension or the first magic bytes.
fn detect_format(source: &InputSource) -> Option<AnimFormat> {
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
        let ms = if denom == 0 { 100 } else { numer / denom };
        let delay = Duration::from_millis(ms.max(20) as u64);
        let image = DynamicImage::ImageRgba8(frame.into_buffer());
        frames.push(AnimFrame { image, delay });
    }
    Ok(frames)
}

/// Decode all frames from an animated image (GIF or WebP).
/// Returns `None` if the source is not an animated format or has ≤1 frame.
pub fn decode_anim_frames(source: &InputSource) -> Result<Option<Vec<AnimFrame>>> {
    let Some(format) = detect_format(source) else {
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

/// Count animation frames without full pixel decoding.
/// Returns None for non-animated sources.
pub fn anim_frame_count(source: &InputSource) -> Option<usize> {
    let format = detect_format(source)?;
    let count = match (source, format) {
        (InputSource::File(path), AnimFormat::Gif) => {
            let reader = BufReader::new(std::fs::File::open(path).ok()?);
            image::codecs::gif::GifDecoder::new(reader).ok()?.into_frames().count()
        }
        (InputSource::File(path), AnimFormat::Webp) => {
            let reader = BufReader::new(std::fs::File::open(path).ok()?);
            image::codecs::webp::WebPDecoder::new(reader).ok()?.into_frames().count()
        }
        (InputSource::Stdin { data }, AnimFormat::Gif) => {
            let reader = Cursor::new(data.clone());
            image::codecs::gif::GifDecoder::new(reader).ok()?.into_frames().count()
        }
        (InputSource::Stdin { data }, AnimFormat::Webp) => {
            let reader = Cursor::new(data.clone());
            image::codecs::webp::WebPDecoder::new(reader).ok()?.into_frames().count()
        }
    };
    if count > 1 { Some(count) } else { None }
}

// ---------------------------------------------------------------------------
// Interactive animated viewer
// ---------------------------------------------------------------------------

/// Interactive animated GIF/WebP viewer. Composes an `AnimationMode` plus
/// the standard Hex / Info / Help auxiliaries and hands the stack to the
/// unified event loop, which drives frame advancement via the mode's
/// `next_tick` / `tick` hooks.
pub fn view_animated(
    source: &InputSource,
    detected: &Detected,
    frames: Vec<AnimFrame>,
    config: ImageConfig,
    initial_theme: PeekThemeName,
) -> Result<()> {
    let mut modes: Vec<Box<dyn Mode>> = Vec::new();
    modes.push(Box::new(AnimationMode::new(frames, config)));
    modes.push(Box::new(HexMode::new(source, 0)?));
    modes.push(Box::new(InfoMode::new()));

    let mut help_actions: Vec<(Action, &'static str)> = GLOBAL_ACTIONS.to_vec();
    for m in &modes {
        for (a, label) in m.extra_actions() {
            if !help_actions.iter().any(|(b, _)| b == a) {
                help_actions.push((*a, *label));
            }
        }
    }
    modes.push(Box::new(HelpMode::new(help_actions)));

    crate::viewer::interactive::run(source, detected, initial_theme, modes)
}

// ---------------------------------------------------------------------------
// Frame rendering (shared with `modes::AnimationMode`)
// ---------------------------------------------------------------------------

pub(crate) fn render_frame(frame: &AnimFrame, config: &ImageConfig) -> Vec<String> {
    use super::glyph_atlas::{CELL_H, CELL_W};

    let mut term = render::TermSize::detect();
    term.rows = term.rows.saturating_sub(1);
    let img = render::add_margin(frame.image.clone(), config.margin);
    let (img_w, img_h) = img.dimensions();
    let (cols, rows) = render::contain_size(img_w, img_h, term, config.width);

    // Resize to target resolution before compositing so checkerboard
    // aligns to the glyph grid.
    let (px_w, px_h) = match config.mode {
        super::ImageMode::Ascii => (cols, rows),
        _ => (cols * CELL_W, rows * CELL_H),
    };
    let img = img.resize_exact(px_w, px_h, image::imageops::FilterType::Lanczos3);
    let img = render::composite_with_bg(img, config.background);

    match config.mode {
        super::ImageMode::Ascii => render::render_density(&img, cols, rows),
        _ => render::render_block_color(&img, cols, rows, config.mode),
    }
}

