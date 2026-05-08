//! Extract a single frame out of a CSS-keyframes SVG animation as a
//! standalone PNG, rasterised at the SVG's native viewport size.
//!
//! Static (non-animated) SVGs return `Unsupported` — the file itself
//! is already standalone, peek the source directly. Animated SVGs are
//! sampled at frame index `key` (1-based to match the user-visible
//! frame counter), the resulting per-frame SVG document is rasterised
//! via resvg at the SVG's intrinsic pixel size, then PNG-encoded into
//! a `Memory`-backed [`InputSource`].

use std::io::Cursor;

use bytes::Bytes;
use image::{ImageEncoder, codecs::png::PngEncoder};

use crate::extract::{ExtractError, Extracted};
use crate::input::InputSource;
use crate::types::image::pipeline::svg;
use crate::types::image::pipeline::svg_anim;

pub fn extract(
    source: &InputSource,
    key: &str,
    size_override: Option<u32>,
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
    let (raster_w, raster_h) = target_dimensions(model.width_px, model.height_px, size_override);
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

/// Minimum pixel size on the longest axis when rasterising an SVG
/// extract without a user-supplied override. Vector SVGs commonly
/// declare a tiny intrinsic size (e.g. `width="24"`) on the
/// assumption that CSS or the consuming app will scale them up —
/// rasterising at the literal intrinsic produces a 24×24 PNG, which
/// is useless as an extract result. Anything below this floor gets
/// scaled up while preserving the aspect ratio; SVGs that already
/// declare a usable intrinsic render at that size.
const SVG_EXTRACT_MIN_DIM: u32 = 512;

/// Resolve the rasterisation target size from an SVG's intrinsic
/// dimensions and an optional user override. The override pins the
/// longest axis to the requested pixel count; without one we fall
/// back to the floor-based default.
fn target_dimensions(intrinsic_w: u32, intrinsic_h: u32, size_override: Option<u32>) -> (u32, u32) {
    let w = intrinsic_w.max(1);
    let h = intrinsic_h.max(1);
    match size_override {
        Some(target) if target > 0 => scale_to_longest_axis(w, h, target),
        _ => {
            let longest = w.max(h);
            if longest >= SVG_EXTRACT_MIN_DIM {
                (w, h)
            } else {
                scale_to_longest_axis(w, h, SVG_EXTRACT_MIN_DIM)
            }
        }
    }
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
        let err = extract(&fixture("unicorn.svg"), "1", None).unwrap_err();
        assert!(matches!(err, ExtractError::Unsupported(_)));
    }

    #[test]
    fn extract_invalid_key_errors() {
        let err = extract(&fixture("loader-dots.svg"), "not-a-number", None).unwrap_err();
        assert!(matches!(err, ExtractError::InvalidKey(_)));
    }

    #[test]
    fn extract_loader_dots_first_frame_is_valid_png() {
        let extracted = extract(&fixture("loader-dots.svg"), "1", None).expect("svg anim extract");
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
            extract(&fixture("loader-dots.svg"), "1", Some(128)).expect("override extract");
        let img = image::load_from_memory(&extracted.source.read_bytes().unwrap()).unwrap();
        assert_eq!(img.width(), 128);
        assert_eq!(img.height(), 128);
    }

    #[test]
    fn target_dimensions_keeps_aspect_when_under_floor() {
        let (w, h) = target_dimensions(24, 12, None);
        assert_eq!(w, SVG_EXTRACT_MIN_DIM);
        assert_eq!(h, SVG_EXTRACT_MIN_DIM / 2);
    }

    #[test]
    fn target_dimensions_passes_through_when_above_floor() {
        let (w, h) = target_dimensions(800, 600, None);
        assert_eq!(w, 800);
        assert_eq!(h, 600);
    }

    #[test]
    fn target_dimensions_with_override_pins_longest_axis() {
        // 100×50 SVG, override = 1024 → longest axis 1024, aspect 2:1.
        let (w, h) = target_dimensions(100, 50, Some(1024));
        assert_eq!(w, 1024);
        assert_eq!(h, 512);
        // Override below intrinsic also takes effect (downscale path).
        let (w, h) = target_dimensions(2000, 1000, Some(256));
        assert_eq!(w, 256);
        assert_eq!(h, 128);
    }
}
