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

pub fn extract(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
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
    let raster = svg::rasterize_svg_bytes(frame_svg.as_bytes(), model.width_px, model.height_px)
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
        let err = extract(&fixture("unicorn.svg"), "1").unwrap_err();
        assert!(matches!(err, ExtractError::Unsupported(_)));
    }

    #[test]
    fn extract_invalid_key_errors() {
        let err = extract(&fixture("loader-dots.svg"), "not-a-number").unwrap_err();
        assert!(matches!(err, ExtractError::InvalidKey(_)));
    }

    #[test]
    fn extract_loader_dots_first_frame_is_valid_png() {
        let extracted = extract(&fixture("loader-dots.svg"), "1").expect("svg anim extract");
        assert!(extracted.suggested_name.ends_with(".png"));
        let bytes = extracted.source.read_bytes().unwrap();
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "expected PNG header"
        );
        let _ = image::load_from_memory(&bytes).expect("PNG should decode");
    }
}
