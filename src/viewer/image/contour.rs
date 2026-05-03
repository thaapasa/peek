//! Edge detection for `ImageMode::Contour`.
//!
//! Sobel 3×3 gradient on luma + Otsu auto-threshold → binary edge image.
//! Output is RGB white-on-black so it flows through `render_block_color`'s
//! 2-cluster path unchanged: edges become the bright cluster, background
//! the dark cluster, glyph picker draws line shapes from the Geo+Block
//! atlas subset.

use image::{DynamicImage, RgbImage};

const EDGE: [u8; 3] = [255, 255, 255];
const VOID: [u8; 3] = [0, 0, 0];

/// Convert an RGB image to a binary edge image.
///
/// Pipeline: luma → Sobel gradient magnitude → Otsu threshold.
/// Pixels at or above threshold become white, others black.
pub fn detect_edges(img: &DynamicImage) -> DynamicImage {
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    if w < 3 || h < 3 {
        return DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb(VOID)));
    }

    let raw = rgb.as_raw();
    let stride = (w * 3) as usize;

    // Luma plane
    let mut luma = vec![0u8; (w * h) as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let off = y * stride + x * 3;
            let l =
                0.299 * raw[off] as f32 + 0.587 * raw[off + 1] as f32 + 0.114 * raw[off + 2] as f32;
            luma[y * w as usize + x] = l as u8;
        }
    }

    // Sobel magnitude (skipping 1-px border).
    let mut mag = vec![0u8; (w * h) as usize];
    let wu = w as usize;
    for y in 1..(h as usize - 1) {
        for x in 1..(w as usize - 1) {
            let p = |dy: isize, dx: isize| {
                let i = ((y as isize + dy) * wu as isize + (x as isize + dx)) as usize;
                luma[i] as i32
            };
            let gx = -p(-1, -1) - 2 * p(0, -1) - p(1, -1) + p(-1, 1) + 2 * p(0, 1) + p(1, 1);
            let gy = -p(-1, -1) - 2 * p(-1, 0) - p(-1, 1) + p(1, -1) + 2 * p(1, 0) + p(1, 1);
            // Approx magnitude; clamp to 0..=255.
            let m = ((gx.abs() + gy.abs()) / 2).min(255) as u8;
            mag[y * wu + x] = m;
        }
    }

    let threshold = otsu(&mag);

    let mut out = RgbImage::new(w, h);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let on = mag[y * wu + x] >= threshold;
            out.put_pixel(x as u32, y as u32, image::Rgb(if on { EDGE } else { VOID }));
        }
    }
    DynamicImage::ImageRgb8(out)
}

/// Otsu's method: pick the threshold that maximizes inter-class variance.
/// Operates on a 256-bin histogram of `data`.
fn otsu(data: &[u8]) -> u8 {
    let mut hist = [0u64; 256];
    for &v in data {
        hist[v as usize] += 1;
    }
    let total: u64 = hist.iter().sum();
    if total == 0 {
        return 128;
    }

    let sum_total: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &c)| i as f64 * c as f64)
        .sum();

    let mut w_bg = 0u64;
    let mut sum_bg = 0.0f64;
    let mut best_var = -1.0f64;
    let mut best_t: u8 = 0;

    for (t, &count) in hist.iter().enumerate() {
        w_bg += count;
        if w_bg == 0 {
            continue;
        }
        let w_fg = total - w_bg;
        if w_fg == 0 {
            break;
        }
        sum_bg += t as f64 * count as f64;
        let mean_bg = sum_bg / w_bg as f64;
        let mean_fg = (sum_total - sum_bg) / w_fg as f64;
        let var = w_bg as f64 * w_fg as f64 * (mean_bg - mean_fg).powi(2);
        if var > best_var {
            best_var = var;
            best_t = t as u8;
        }
    }

    // Floor at a small value so completely flat images don't pick 0
    // (which would mark every pixel as an edge).
    best_t.max(16)
}
