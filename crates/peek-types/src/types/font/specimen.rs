//! Rasterise a hard-coded sample string into a `DynamicImage` for the
//! existing image pipeline to convert to ASCII art.
//!
//! Strategy:
//!  1. Layout the sample text at a fixed pixel size — fontdue gives
//!     per-glyph metrics and coverage maps for any TTF/OTF face.
//!  2. Blit each glyph into an RGBA8 buffer at baseline-aligned
//!     positions.
//!  3. Return the buffer; `prepare_decoded` handles the resize +
//!     composite step downstream, and the existing image-config cycle
//!     (block / geo / ascii / contour) all "just work" because the
//!     glyph specimen lands as an ordinary raster.

use anyhow::{Result, anyhow};
use fontdue::{Font, FontSettings};
use image::{DynamicImage, RgbaImage};

/// Sample text shown for every font specimen. Two pangram-style lines
/// (Latin coverage), digits + common punctuation, a mixed-case ASCII
/// alphabet line. Hard-coded ASCII for now — multi-script samplers
/// (Cyrillic / Greek / etc.) belong in a later phase that consults
/// cmap coverage to pick the sampler.
const SAMPLE_LINES: &[&str] = &[
    "The quick brown fox",
    "jumps over the lazy dog",
    "0123456789  !?@#$%&*()",
    "AaBbCcDdEeFfGgHhIiJjKkLl",
];

/// Rasterise the sample text into an RGBA8 image. `face_index` picks a
/// face out of a TTF / OTF / TTC container — pass `0` for plain
/// single-face fonts.
///
/// `target_height_px` is the desired total canvas height; the per-line
/// font size is derived from that so the specimen fits the requested
/// vertical budget regardless of the sample's line count. Width is
/// computed from whichever sample line draws widest.
pub fn rasterise(bytes: &[u8], face_index: u32, target_height_px: u32) -> Result<DynamicImage> {
    let settings = FontSettings {
        collection_index: face_index,
        ..FontSettings::default()
    };
    let font = Font::from_bytes(bytes, settings).map_err(|e| anyhow!("fontdue: {e}"))?;

    // Per-line size: budget the canvas height across the sample line
    // count plus one leading-and-trailing line of padding. Cap at a
    // generous max so an absurd `target_height_px` doesn't ask for a
    // glyph bigger than fontdue likes to rasterise.
    let lines = SAMPLE_LINES.len() as u32;
    let line_count_with_padding = lines.saturating_add(1).max(1);
    let mut px_per_line = (target_height_px / line_count_with_padding).max(8);
    px_per_line = px_per_line.min(96);
    let font_size = px_per_line as f32;
    let line_height = (font_size * 1.4) as u32; // vertical advance + a touch of leading

    // Two-pass layout: first compute the canvas size; then allocate and
    // blit. Avoids growing the buffer per glyph.
    let mut widest: u32 = 0;
    let mut measured: Vec<Vec<(fontdue::Metrics, Vec<u8>)>> =
        Vec::with_capacity(SAMPLE_LINES.len());
    for line in SAMPLE_LINES {
        let mut x: i32 = 0;
        let mut row: Vec<(fontdue::Metrics, Vec<u8>)> = Vec::with_capacity(line.len());
        for ch in line.chars() {
            let (metrics, bitmap) = font.rasterize(ch, font_size);
            x += metrics.advance_width as i32;
            row.push((metrics, bitmap));
        }
        if (x as u32) > widest {
            widest = x as u32;
        }
        measured.push(row);
    }

    let margin = (font_size * 0.5) as u32;
    let canvas_w = widest.saturating_add(margin * 2).max(1);
    let canvas_h = (line_height * lines).saturating_add(margin * 2).max(1);

    let mut canvas = RgbaImage::new(canvas_w, canvas_h);
    // Solid white background — the image pipeline's auto-background
    // detection then sees an opaque image and skips the alpha-composite
    // step, which preserves the glyph edges. The chosen background
    // also makes the specimen readable in any terminal palette.
    for px in canvas.pixels_mut() {
        *px = image::Rgba([255, 255, 255, 255]);
    }

    // Baseline placement: fontdue's metrics give glyph height and
    // `ymin` (rows below baseline). Anchor each line's baseline at
    // 80% of the line's vertical slot so descenders have room.
    for (line_idx, row) in measured.iter().enumerate() {
        let baseline_y =
            margin as i32 + (line_idx as i32) * line_height as i32 + (font_size * 0.8) as i32;
        let mut pen_x: i32 = margin as i32;
        for (metrics, bitmap) in row {
            let glyph_w = metrics.width as i32;
            let glyph_h = metrics.height as i32;
            let glyph_top = baseline_y - (metrics.height as i32 + metrics.ymin);
            let glyph_left = pen_x + metrics.xmin;
            for gy in 0..glyph_h {
                for gx in 0..glyph_w {
                    let coverage = bitmap[(gy * glyph_w + gx) as usize];
                    if coverage == 0 {
                        continue;
                    }
                    let cx = glyph_left + gx;
                    let cy = glyph_top + gy;
                    if cx < 0 || cy < 0 || cx >= canvas_w as i32 || cy >= canvas_h as i32 {
                        continue;
                    }
                    // Coverage = grayscale ink. Compose against the
                    // white background by darkening per coverage value.
                    let dst = canvas.get_pixel_mut(cx as u32, cy as u32);
                    let inv = 255 - coverage;
                    dst[0] = inv;
                    dst[1] = inv;
                    dst[2] = inv;
                }
            }
            pen_x += metrics.advance_width as i32;
        }
    }

    Ok(DynamicImage::ImageRgba8(canvas))
}
