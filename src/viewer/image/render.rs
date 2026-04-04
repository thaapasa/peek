use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};

use super::clustering::fast_2_color;
use super::glyph_atlas::{atlas_for_mode, best_glyph, GlyphBitmap, CELL_H, CELL_W};
use super::ImageMode;

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
    let rows_from_width =
        (img_h as f64 * term.cols as f64 / (img_w as f64 * 2.0)) as u32;

    if rows_from_width <= term.rows {
        // Fits vertically — use full width
        (term.cols, rows_from_width.max(1))
    } else {
        // Too tall — fit to terminal height instead
        let cols_from_height =
            (img_w as f64 * term.rows as f64 * 2.0 / img_h as f64) as u32;
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
) -> Vec<String> {
    let px_w = term_cols * CELL_W;
    let px_h = term_rows * CELL_H;
    let resized = img
        .resize_exact(px_w, px_h, image::imageops::FilterType::Lanczos3)
        .to_rgb8();

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
                    cell_pixels[cy * CELL_W as usize + cx] = [
                        raw[px_offset],
                        raw[px_offset + 1],
                        raw[px_offset + 2],
                    ];
                }
            }

            let cluster = fast_2_color(&cell_pixels);
            let glyph_match = best_glyph(cluster.bitmap, &atlas);

            let (fg, bg) = if glyph_match.inverted {
                (cluster.color_b, cluster.color_a)
            } else {
                (cluster.color_a, cluster.color_b)
            };

            line.push_str(&format!(
                "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m{}",
                fg[0], fg[1], fg[2], bg[0], bg[1], bg[2], glyph_match.ch
            ));
        }

        line.push_str("\x1b[0m");
        lines.push(line);
    }

    lines
}

/// Render an image using the legacy density-ramp algorithm.
/// Returns a vector of ANSI-colored lines.
pub fn render_density(img: &DynamicImage, term_cols: u32, term_rows: u32) -> Vec<String> {
    const DENSITY_RAMP: &[u8] =
        b" .'`^\",:;Il!i><~+_-?][}{1)(|/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$";

    let resized = img.resize_exact(
        term_cols,
        term_rows,
        image::imageops::FilterType::Lanczos3,
    );

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

            line.push_str(&format!("\x1b[38;2;{r};{g};{b}m{ch}"));
        }
        line.push_str("\x1b[0m");
        lines.push(line);
    }

    lines
}

/// Load and render an image to lines, using contain-ratio sizing.
pub fn load_and_render(
    path: &std::path::Path,
    mode: ImageMode,
    forced_width: u32,
    term: TermSize,
) -> Result<Vec<String>> {
    let img = image::open(path).context("failed to open image")?;
    let (img_w, img_h) = img.dimensions();
    let (cols, rows) = contain_size(img_w, img_h, term, forced_width);

    let lines = match mode {
        ImageMode::Ascii => render_density(&img, cols, rows),
        ImageMode::Full | ImageMode::Block => render_block_color(&img, cols, rows, mode),
    };

    Ok(lines)
}
