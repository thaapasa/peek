use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};

use super::clustering::fast_2_color;
use super::glyph_atlas::{CELL_H, CELL_W, GlyphBitmap, atlas_for_mode, best_glyph};
use super::{Background, ImageConfig, ImageMode};
use crate::input::InputSource;
use crate::theme::ColorMode;

/// Terminal dimensions in characters.
#[derive(Debug, Clone, Copy)]
pub struct TermSize {
    pub cols: u32,
    pub rows: u32,
}

impl TermSize {
    pub fn detect() -> Self {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        Self {
            cols: cols as u32,
            rows: rows as u32,
        }
    }
}

/// Compute the output grid size (cols, rows) for an image that fits
/// within the given terminal dimensions while preserving aspect ratio.
///
/// Uses "contain" logic: the image is scaled to fit entirely within
/// the terminal, constrained by both width and height.
pub fn contain_size(img_w: u32, img_h: u32, term: TermSize, forced_width: u32) -> (u32, u32) {
    if forced_width > 0 {
        // Forced width: only constrain by width, ignore height
        let rows = (img_h as f64 * forced_width as f64 / (img_w as f64 * 2.0)) as u32;
        return (forced_width, rows.max(1));
    }

    // Aspect ratio rule for terminal cells (~2:1 height:width):
    //   cols / (rows * 2) = img_w / img_h
    // So:
    //   rows = img_h * cols / (img_w * 2)
    //   cols = img_w * rows * 2 / img_h

    // Try fitting to terminal width
    let rows_from_width = (img_h as f64 * term.cols as f64 / (img_w as f64 * 2.0)) as u32;

    if rows_from_width <= term.rows {
        // Fits vertically — use full width
        (term.cols, rows_from_width.max(1))
    } else {
        // Too tall — fit to terminal height instead
        let cols_from_height = (img_w as f64 * term.rows as f64 * 2.0 / img_h as f64) as u32;
        (cols_from_height.clamp(1, term.cols), term.rows)
    }
}

/// Render an image using the block-color algorithm.
/// Returns a vector of ANSI-colored lines.
pub fn render_block_color(
    img: &DynamicImage,
    term_cols: u32,
    term_rows: u32,
    mode: ImageMode,
    color_mode: ColorMode,
) -> Vec<String> {
    let px_w = term_cols * CELL_W;
    let px_h = term_rows * CELL_H;
    let resized = if img.width() == px_w && img.height() == px_h {
        img.to_rgb8()
    } else {
        img.resize_exact(px_w, px_h, image::imageops::FilterType::Lanczos3)
            .to_rgb8()
    };

    let raw = resized.as_raw();
    let stride = (px_w * 3) as usize;

    let atlas_refs = atlas_for_mode(mode);
    let atlas: Vec<GlyphBitmap> = atlas_refs.iter().map(|g| **g).collect();

    let mut cell_pixels = [[0u8; 3]; 128];
    let mut lines = Vec::with_capacity(term_rows as usize);

    for row in 0..term_rows {
        let mut line = String::with_capacity((term_cols * 40) as usize);

        for col in 0..term_cols {
            let base_x = (col * CELL_W) as usize;
            let base_y = (row * CELL_H) as usize;

            for cy in 0..CELL_H as usize {
                for cx in 0..CELL_W as usize {
                    let px_offset = (base_y + cy) * stride + (base_x + cx) * 3;
                    cell_pixels[cy * CELL_W as usize + cx] =
                        [raw[px_offset], raw[px_offset + 1], raw[px_offset + 2]];
                }
            }

            let cluster = fast_2_color(&cell_pixels);
            let glyph_match = best_glyph(cluster.bitmap, &atlas);

            let (fg, bg) = if glyph_match.inverted {
                (cluster.color_b, cluster.color_a)
            } else {
                (cluster.color_a, cluster.color_b)
            };

            color_mode.write_fg_bg(&mut line, fg, bg, glyph_match.ch);
        }

        line.push_str(color_mode.reset());
        lines.push(line);
    }

    lines
}

/// Render an image using the legacy density-ramp algorithm.
/// Returns a vector of ANSI-colored lines.
pub fn render_density(
    img: &DynamicImage,
    term_cols: u32,
    term_rows: u32,
    color_mode: ColorMode,
) -> Vec<String> {
    const DENSITY_RAMP: &[u8] =
        b" .'`^\",:;Il!i><~+_-?][}{1)(|/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$";

    let resized = if img.width() == term_cols && img.height() == term_rows {
        img.clone()
    } else {
        img.resize_exact(term_cols, term_rows, image::imageops::FilterType::Lanczos3)
    };

    let ramp_len = DENSITY_RAMP.len();
    let mut lines = Vec::with_capacity(term_rows as usize);

    for y in 0..term_rows {
        let mut line = String::with_capacity((term_cols * 20) as usize);
        for x in 0..term_cols {
            let pixel = resized.get_pixel(x, y);
            let [r, g, b, _a] = pixel.0;

            let luma = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
            let idx = ((luma / 255.0) * (ramp_len - 1) as f64) as usize;
            let ch = DENSITY_RAMP[idx.min(ramp_len - 1)] as char;

            color_mode.write_fg(&mut line, [r, g, b], ch);
        }
        line.push_str(color_mode.reset());
        lines.push(line);
    }

    lines
}

/// Add transparent margin around an image.
pub fn add_margin(img: DynamicImage, margin: u32) -> DynamicImage {
    if margin == 0 {
        return img;
    }
    let (w, h) = img.dimensions();
    // Canvas is initialized to [0,0,0,0] (fully transparent).
    let mut canvas = image::RgbaImage::new(w + margin * 2, h + margin * 2);
    image::imageops::overlay(&mut canvas, &img.to_rgba8(), margin as i64, margin as i64);
    DynamicImage::ImageRgba8(canvas)
}

/// Check if an image has an alpha channel.
fn has_alpha(img: &DynamicImage) -> bool {
    use image::ColorType;
    matches!(
        img.color(),
        ColorType::Rgba8
            | ColorType::Rgba16
            | ColorType::Rgba32F
            | ColorType::La8
            | ColorType::La16
    )
}

/// Analyze non-transparent pixels to choose a compositing background.
/// Dark content → white background, light content → black background.
fn auto_background(img: &DynamicImage) -> [u8; 3] {
    let rgba = img.to_rgba8();
    let (mut luma_sum, mut count) = (0.0f64, 0u64);
    for pixel in rgba.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 10 {
            continue;
        }
        luma_sum += 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
        count += 1;
    }
    if count == 0 {
        return [255, 255, 255];
    }
    if luma_sum / (count as f64) < 128.0 {
        [255, 255, 255]
    } else {
        [0, 0, 0]
    }
}

/// Resolve a Background setting to an RGB color for a given pixel position.
fn resolve_bg(bg: Background, img: &DynamicImage) -> Box<dyn Fn(u32, u32) -> [u8; 3]> {
    match bg {
        Background::Auto => {
            let color = if has_alpha(img) {
                auto_background(img)
            } else {
                [0, 0, 0]
            };
            Box::new(move |_x, _y| color)
        }
        Background::Black => Box::new(|_x, _y| [0, 0, 0]),
        Background::White => Box::new(|_x, _y| [255, 255, 255]),
        Background::Checkerboard => {
            // Half-block-sized checkerboard (8x8 px = one half-block glyph)
            Box::new(|x, y| {
                let cell = (x / 8 + y / 8) % 2;
                if cell == 0 {
                    [204, 204, 204]
                } else {
                    [102, 102, 102]
                }
            })
        }
    }
}

/// Composite an RGBA image against a background, returning an RGB image.
fn composite_onto(img: &DynamicImage, bg_fn: &dyn Fn(u32, u32) -> [u8; 3]) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut rgb = image::RgbImage::new(w, h);
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = a as f32 / 255.0;
        let inv = 1.0 - alpha;
        let bg = bg_fn(x, y);
        rgb.put_pixel(
            x,
            y,
            image::Rgb([
                (r as f32 * alpha + bg[0] as f32 * inv) as u8,
                (g as f32 * alpha + bg[1] as f32 * inv) as u8,
                (b as f32 * alpha + bg[2] as f32 * inv) as u8,
            ]),
        );
    }
    DynamicImage::ImageRgb8(rgb)
}

/// Apply alpha compositing with the given background mode.
pub fn composite_with_bg(img: DynamicImage, bg: Background) -> DynamicImage {
    if !has_alpha(&img) && bg == Background::Auto {
        return img;
    }
    let bg_fn = resolve_bg(bg, &img);
    composite_onto(&img, &*bg_fn)
}

/// Load and render an image to lines, using contain-ratio sizing.
/// Resizes to target resolution before compositing so that the checkerboard
/// pattern is always aligned to the glyph grid.
pub fn load_and_render(
    source: &InputSource,
    config: &ImageConfig,
    term: TermSize,
) -> Result<Vec<String>> {
    Ok(render_decoded(load_image(source)?, config, term))
}

/// Render an already-decoded image. Shared by `load_and_render` and the
/// animation loop, which holds frames in memory and shouldn't re-decode.
///
/// Resizes to target pixel resolution before alpha-compositing so the
/// checkerboard pattern aligns to the glyph grid.
pub fn render_decoded(img: DynamicImage, config: &ImageConfig, term: TermSize) -> Vec<String> {
    let img = add_margin(img, config.margin);
    let (img_w, img_h) = img.dimensions();
    let (cols, rows) = contain_size(img_w, img_h, term, config.width);

    let (px_w, px_h) = match config.mode {
        ImageMode::Ascii => (cols, rows),
        _ => (cols * CELL_W, rows * CELL_H),
    };
    let img = img.resize_exact(px_w, px_h, image::imageops::FilterType::Lanczos3);
    let img = composite_with_bg(img, config.background);

    match config.mode {
        ImageMode::Ascii => render_density(&img, cols, rows, config.color_mode),
        ImageMode::Full | ImageMode::Block | ImageMode::Geo => {
            render_block_color(&img, cols, rows, config.mode, config.color_mode)
        }
    }
}

/// Load an image from a File path or buffered Stdin bytes.
pub fn load_image(source: &InputSource) -> Result<DynamicImage> {
    match source {
        InputSource::File(path) => image::open(path).context("failed to open image"),
        InputSource::Stdin { data } => {
            image::load_from_memory(data).context("failed to decode image from stdin")
        }
    }
}

/// Load and render an SVG source to ASCII art lines.
/// Rasterizes at the exact target pixel resolution for maximum sharpness.
pub fn load_and_render_svg(
    source: &InputSource,
    config: &ImageConfig,
    term: TermSize,
) -> Result<Vec<String>> {
    let (svg_w, svg_h) = super::svg::svg_dimensions(source)?;
    let margin = config.margin;
    // Account for margin in aspect ratio calculation
    let padded_w = svg_w + margin * 2;
    let padded_h = svg_h + margin * 2;
    let (cols, rows) = contain_size(padded_w, padded_h, term, config.width);

    // Compute target pixel size, then rasterize SVG into the inner area
    // (target minus margin) so that adding margin reaches exact target size.
    let (px_w, px_h) = match config.mode {
        ImageMode::Ascii => (cols, rows),
        _ => (cols * CELL_W, rows * CELL_H),
    };
    let scale_x = px_w as f64 / padded_w as f64;
    let scale_y = px_h as f64 / padded_h as f64;
    let target_margin_x = (margin as f64 * scale_x).round() as u32;
    let target_margin_y = (margin as f64 * scale_y).round() as u32;
    let inner_w = px_w.saturating_sub(target_margin_x * 2).max(1);
    let inner_h = px_h.saturating_sub(target_margin_y * 2).max(1);

    let inner = super::svg::rasterize_svg(source, inner_w, inner_h)?;
    // Place the SVG content centered in a full-size transparent canvas
    let mut canvas = image::RgbaImage::new(px_w, px_h);
    let offset_x = (px_w - inner_w) / 2;
    let offset_y = (px_h - inner_h) / 2;
    image::imageops::overlay(
        &mut canvas,
        &inner.to_rgba8(),
        offset_x as i64,
        offset_y as i64,
    );
    let img = DynamicImage::ImageRgba8(canvas);
    let img = composite_with_bg(img, config.background);

    let lines = match config.mode {
        ImageMode::Ascii => render_density(&img, cols, rows, config.color_mode),
        _ => render_block_color(&img, cols, rows, config.mode, config.color_mode),
    };

    Ok(lines)
}
