//! Extract a CSS-keyframes SVG animation frame as a PNG. Sample frame
//! N (1-based) via the existing svg_anim parser, rasterise via resvg
//! at the SVG's intrinsic size (sub-floor sizes scale up to keep the
//! output usable), PNG-encode into a Memory-backed [`InputSource`].
//! Non-animated SVGs return `Unsupported`.

use std::io::Cursor;

use bytes::Bytes;
use image::{ImageEncoder, codecs::png::PngEncoder};

use crate::extract::{ExtractError, Extracted};
use crate::input::InputSource;
use crate::types::image::pipeline::glyph_atlas::CELL_W;
use crate::types::image::pipeline::svg;
use crate::types::image::pipeline::svg_anim;

pub fn extract(
    source: &InputSource,
    key: &str,
    size_override: Option<u32>,
    view_cols: Option<u32>,
) -> Result<Extracted, ExtractError> {
    let model = svg_anim::try_parse(source)
        .map_err(ExtractError::Other)?
        .ok_or(ExtractError::Unsupported(
            "SVG is not animated; the file itself is already a single image",
        ))?;

    let total = model.frames.len();
    let one_based: usize = key
        .parse()
        .map_err(|_| ExtractError::InvalidKey(key.to_string()))?;
    if one_based == 0 || one_based > total {
        return Err(ExtractError::NotFound(format!(
            "frame {one_based} out of range 1..={total}"
        )));
    }
    let idx = one_based - 1;

    let frame_svg = svg_anim::render_frame(&model, idx);
    let (raster_w, raster_h) =
        target_dimensions(model.width_px, model.height_px, size_override, view_cols);
    let raster = svg::rasterize_svg_bytes(frame_svg.as_bytes(), raster_w, raster_h)
        .map_err(ExtractError::Other)?;

    let rgba = raster.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut buf = Vec::with_capacity((w as usize) * (h as usize) * 4);
    PngEncoder::new(Cursor::new(&mut buf))
        .write_image(rgba.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| ExtractError::Other(anyhow::Error::from(e)))?;

    let suggested_name = suggest_name(source, one_based, total);
    Ok(Extracted {
        source: InputSource::memory(Bytes::from(buf), suggested_name.clone()),
        suggested_name,
    })
}

/// Floor (longest axis, px) applied when no override is given. SVGs
/// often declare tiny intrinsic sizes (`width="24"`) expecting CSS to
/// scale them; literal-intrinsic raster would make extracts useless.
const SVG_EXTRACT_MIN_DIM: u32 = 512;

/// Resolution priority: explicit `size_override` (longest-axis pin) →
/// `view_cols` (match what live render at that char width would
/// produce) → intrinsic when above floor → upscale to floor.
fn target_dimensions(
    intrinsic_w: u32,
    intrinsic_h: u32,
    size_override: Option<u32>,
    view_cols: Option<u32>,
) -> (u32, u32) {
    let w = intrinsic_w.max(1);
    let h = intrinsic_h.max(1);
    if let Some(target) = size_override
        && target > 0
    {
        return scale_to_longest_axis(w, h, target);
    }
    if let Some(cols) = view_cols
        && cols > 0
    {
        return raster_for_view_cols(w, h, cols);
    }
    let longest = w.max(h);
    if longest >= SVG_EXTRACT_MIN_DIM {
        (w, h)
    } else {
        scale_to_longest_axis(w, h, SVG_EXTRACT_MIN_DIM)
    }
}

/// Raster size that matches a live render at `view_cols` character
/// columns. Live render scales the SVG so the prepared grid is
/// `cols × rows` cells where `rows = h * cols / (w * 2)`, then
/// rasters at `cols * CELL_W × rows * CELL_H` pixels. Aspect ratio
/// of the result equals the SVG's intrinsic aspect ratio (the cell
/// 2:1 ratio in `rows` cancels with `CELL_H = 2*CELL_W`), so this
/// reduces to `(view_cols * CELL_W, view_cols * CELL_W * h / w)`.
fn raster_for_view_cols(w: u32, h: u32, view_cols: u32) -> (u32, u32) {
    let raster_w = view_cols.saturating_mul(CELL_W).max(1);
    let raster_h = ((raster_w as u64 * h as u64 / w as u64) as u32).max(1);
    (raster_w, raster_h)
}

fn scale_to_longest_axis(w: u32, h: u32, target_longest: u32) -> (u32, u32) {
    let longest = w.max(h);
    let scale = target_longest as f64 / longest as f64;
    let sw = ((w as f64 * scale).round() as u32).max(1);
    let sh = ((h as f64 * scale).round() as u32).max(1);
    (sw, sh)
}

fn suggest_name(source: &InputSource, frame_one_based: usize, total: usize) -> String {
    let stem = source
        .path()
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            source
                .name()
                .trim_matches(|c: char| matches!(c, '<' | '>'))
                .to_string()
        });
    let stem = if stem.is_empty() {
        "frame".to_string()
    } else {
        stem
    };
    let width = total.to_string().len();
    format!("{stem}-frame-{frame_one_based:0width$}.png")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("test-images");
        p.push(name);
        InputSource::File(p)
    }

    #[test]
    fn extract_static_svg_unsupported() {
        // unicorn.svg is a non-animated SVG.
        let err = extract(&fixture("unicorn.svg"), "1", None, None).unwrap_err();
        assert!(matches!(err, ExtractError::Unsupported(_)));
    }

    #[test]
    fn extract_invalid_key_errors() {
        let err = extract(&fixture("loader-dots.svg"), "not-a-number", None, None).unwrap_err();
        assert!(matches!(err, ExtractError::InvalidKey(_)));
    }

    #[test]
    fn extract_loader_dots_first_frame_is_valid_png() {
        let extracted =
            extract(&fixture("loader-dots.svg"), "1", None, None).expect("svg anim extract");
        assert!(extracted.suggested_name.ends_with(".png"));
        let bytes = extracted.source.read_bytes().unwrap();
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "expected PNG header"
        );
        let img = image::load_from_memory(&bytes).expect("PNG should decode");
        // loader-dots declares width=24 height=24 intrinsic — extract
        // upscales to the SVG floor so the saved PNG is actually
        // useful rather than a 24-px thumbnail.
        assert_eq!(img.width(), SVG_EXTRACT_MIN_DIM);
        assert_eq!(img.height(), SVG_EXTRACT_MIN_DIM);
    }

    #[test]
    fn extract_honours_size_override() {
        let extracted =
            extract(&fixture("loader-dots.svg"), "1", Some(128), None).expect("override extract");
        let img = image::load_from_memory(&extracted.source.read_bytes().unwrap()).unwrap();
        assert_eq!(img.width(), 128);
        assert_eq!(img.height(), 128);
    }

    #[test]
    fn view_cols_matches_live_render_at_same_width() {
        // 24×24 SVG, view_cols = 200 → raster = 200*CELL_W × same.
        let extracted =
            extract(&fixture("loader-dots.svg"), "1", None, Some(200)).expect("view_cols extract");
        let img = image::load_from_memory(&extracted.source.read_bytes().unwrap()).unwrap();
        assert_eq!(img.width(), 200 * CELL_W);
        assert_eq!(img.height(), 200 * CELL_W);
    }

    #[test]
    fn size_override_wins_over_view_cols() {
        let (w, h) = target_dimensions(24, 24, Some(64), Some(200));
        assert_eq!(w, 64);
        assert_eq!(h, 64);
    }

    #[test]
    fn raster_for_view_cols_preserves_intrinsic_aspect() {
        // Square: view_cols 100 → 100*CELL_W square.
        let (w, h) = raster_for_view_cols(24, 24, 100);
        assert_eq!(w, 100 * CELL_W);
        assert_eq!(h, 100 * CELL_W);
        // Wide 2:1.
        let (w, h) = raster_for_view_cols(48, 24, 100);
        assert_eq!(w, 100 * CELL_W);
        assert_eq!(h, 100 * CELL_W / 2);
        // Tall 1:2.
        let (w, h) = raster_for_view_cols(24, 48, 100);
        assert_eq!(w, 100 * CELL_W);
        assert_eq!(h, 100 * CELL_W * 2);
    }

    #[test]
    fn target_dimensions_keeps_aspect_when_under_floor() {
        let (w, h) = target_dimensions(24, 12, None, None);
        assert_eq!(w, SVG_EXTRACT_MIN_DIM);
        assert_eq!(h, SVG_EXTRACT_MIN_DIM / 2);
    }

    #[test]
    fn target_dimensions_passes_through_when_above_floor() {
        let (w, h) = target_dimensions(800, 600, None, None);
        assert_eq!(w, 800);
        assert_eq!(h, 600);
    }

    #[test]
    fn target_dimensions_with_override_pins_longest_axis() {
        // 100×50 SVG, override = 1024 → longest axis 1024, aspect 2:1.
        let (w, h) = target_dimensions(100, 50, Some(1024), None);
        assert_eq!(w, 1024);
        assert_eq!(h, 512);
        // Override below intrinsic also takes effect (downscale path).
        let (w, h) = target_dimensions(2000, 1000, Some(256), None);
        assert_eq!(w, 256);
        assert_eq!(h, 128);
    }
}
