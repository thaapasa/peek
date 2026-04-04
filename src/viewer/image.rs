use std::path::Path;

use anyhow::{Context, Result};
use image::GenericImageView;

use crate::detect::FileType;
use crate::pager::Output;

use super::Viewer;

/// ASCII characters ordered by visual density (light → dark).
/// Each character's glyph occupies a different amount of the cell,
/// giving the illusion of brightness levels.
const DENSITY_RAMP: &[u8] = b" .'`^\",:;Il!i><~+_-?][}{1)(|/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$";

pub struct ImageViewer {
    width: u32,
}

impl ImageViewer {
    pub fn new(width: u32) -> Self {
        Self { width }
    }

    fn terminal_width(&self) -> u32 {
        if self.width > 0 {
            return self.width;
        }
        let (cols, _rows) = crossterm::terminal::size().unwrap_or((80, 24));
        cols as u32
    }
}

impl Viewer for ImageViewer {
    fn render(&self, path: &Path, _file_type: &FileType, output: &mut Output) -> Result<()> {
        let img = image::open(path).context("failed to open image")?;

        let term_width = self.terminal_width();

        // Terminal characters are roughly twice as tall as they are wide,
        // so we scale height by 0.5 to maintain aspect ratio.
        let (img_w, img_h) = img.dimensions();
        let scale = term_width as f64 / img_w as f64;
        let new_w = term_width;
        let new_h = (img_h as f64 * scale * 0.5) as u32;

        let resized = img.resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3);

        let ramp_len = DENSITY_RAMP.len();

        for y in 0..new_h {
            let mut line = String::with_capacity((new_w * 20) as usize);
            for x in 0..new_w {
                let pixel = resized.get_pixel(x, y);
                let [r, g, b, _a] = pixel.0;

                // Perceived luminance (ITU-R BT.601)
                let luma = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
                let idx = ((luma / 255.0) * (ramp_len - 1) as f64) as usize;
                let ch = DENSITY_RAMP[idx.min(ramp_len - 1)] as char;

                // True color (24-bit) ANSI escape for foreground
                line.push_str(&format!("\x1b[38;2;{r};{g};{b}m{ch}"));
            }
            line.push_str("\x1b[0m");
            output.write_line(&line)?;
        }

        Ok(())
    }
}
