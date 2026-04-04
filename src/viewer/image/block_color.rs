use std::path::Path;

use anyhow::{Context, Result};
use image::GenericImageView;

use crate::detect::FileType;
use crate::pager::Output;

use super::glyph_atlas::{atlas_for_mode, best_glyph, GlyphBitmap, CELL_H, CELL_W};
use super::clustering::fast_2_color;
use super::{ImageMode, Viewer};

/// Two-color glyph-matched image renderer.
///
/// For each character cell, finds two dominant colors and selects the glyph
/// whose shape best matches the spatial distribution of those colors.
/// Uses both foreground and background terminal colors.
pub struct BlockColorRenderer {
    width: u32,
    mode: ImageMode,
}

impl BlockColorRenderer {
    pub fn new(width: u32, mode: ImageMode) -> Self {
        Self { width, mode }
    }

    fn terminal_width(&self) -> u32 {
        if self.width > 0 {
            return self.width;
        }
        let (cols, _rows) = crossterm::terminal::size().unwrap_or((80, 24));
        cols as u32
    }
}

impl Viewer for BlockColorRenderer {
    fn render(&self, path: &Path, _file_type: &FileType, output: &mut Output) -> Result<()> {
        let img = image::open(path).context("failed to open image")?;

        let term_width = self.terminal_width();

        // Compute grid dimensions preserving aspect ratio.
        // Terminal characters are ~2:1 (height:width), so we halve the row count.
        let (img_w, img_h) = img.dimensions();
        let scale = term_width as f64 / img_w as f64;
        let term_rows = ((img_h as f64 * scale * 0.5) as u32).max(1);
        let term_cols = term_width;

        // Resize to the sub-pixel grid: each cell is CELL_W x CELL_H pixels
        let px_w = term_cols * CELL_W;
        let px_h = term_rows * CELL_H;
        let resized = img
            .resize_exact(px_w, px_h, image::imageops::FilterType::Lanczos3)
            .to_rgb8();

        let raw = resized.as_raw();
        let stride = (px_w * 3) as usize; // bytes per row in the raw buffer

        // Get the glyph atlas for the current mode
        let atlas_refs = atlas_for_mode(self.mode);
        let atlas: Vec<GlyphBitmap> = atlas_refs.iter().map(|g| **g).collect();

        // Reusable pixel buffer for one cell
        let mut cell_pixels = [[0u8; 3]; 128];

        for row in 0..term_rows {
            let mut line = String::with_capacity((term_cols * 40) as usize);

            for col in 0..term_cols {
                // Extract CELL_W x CELL_H pixel block for this cell
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

                // Cluster into 2 colors
                let cluster = fast_2_color(&cell_pixels);

                // Find best matching glyph
                let glyph_match = best_glyph(cluster.bitmap, &atlas);

                // Determine fg/bg colors based on whether the match is inverted
                let (fg, bg) = if glyph_match.inverted {
                    (cluster.color_b, cluster.color_a)
                } else {
                    (cluster.color_a, cluster.color_b)
                };

                // Emit ANSI: set fg + bg + glyph character
                line.push_str(&format!(
                    "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m{}",
                    fg[0], fg[1], fg[2], bg[0], bg[1], bg[2], glyph_match.ch
                ));
            }

            line.push_str("\x1b[0m");
            output.write_line(&line)?;
        }

        Ok(())
    }
}
