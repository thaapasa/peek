//! Edge detection for `ImageMode::Contour`.
//!
//! Sobel 3×3 gradient on luma + percentile threshold → binary edge image.
//! Output is white-on-black RGB consumed by `render_contour`.
//!
//! Threshold is percentile-based rather than Otsu so a fixed fraction of
//! pixels is marked as edges regardless of frame content. Otsu's bimodal
//! assumption breaks on smooth-content frames (faces, sky), making
//! animation flicker badly as the chosen threshold whips around between
//! frames.

use image::{DynamicImage, RgbImage};

const EDGE: [u8; 3] = [255, 255, 255];
const VOID: [u8; 3] = [0, 0, 0];

/// Convert an RGB image to a binary edge image.
///
/// Pipeline: luma → Sobel gradient magnitude → percentile threshold.
/// `density` is the target fraction (0.0..1.0) of pixels above the
/// threshold; higher means denser line-art. Pixels at or above the
/// resolved threshold become white, others black.
pub fn detect_edges(img: &DynamicImage, density: f32) -> DynamicImage {
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

    let threshold = percentile_threshold(&mag, density);

    let mut out = RgbImage::new(w, h);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let on = mag[y * wu + x] >= threshold;
            out.put_pixel(x as u32, y as u32, image::Rgb(if on { EDGE } else { VOID }));
        }
    }
    DynamicImage::ImageRgb8(out)
}

/// Percentile threshold: pick the value above which roughly `density`
/// fraction of pixels lie. Walks a 256-bin histogram from the top until
/// the running count reaches the target.
///
/// Floors at 8 so the threshold never collapses into the noise-floor
/// (gradient ≈ 0 for flat regions of the image).
fn percentile_threshold(data: &[u8], density: f32) -> u8 {
    let density = density.clamp(0.001, 0.99);
    let mut hist = [0u64; 256];
    for &v in data {
        hist[v as usize] += 1;
    }
    let total: u64 = hist.iter().sum();
    if total == 0 {
        return 128;
    }

    let target = (density * total as f32).max(1.0) as u64;
    let mut cum = 0u64;
    for t in (0..256).rev() {
        cum += hist[t];
        if cum >= target {
            return (t as u8).max(8);
        }
    }
    8
}
