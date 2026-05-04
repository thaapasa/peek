//! SVG rasterization helpers used by the image rendering pipeline. The
//! interactive view of an SVG flows through `ImageRenderMode` (which
//! calls `render::prepare_svg`); these helpers are the resvg-backed
//! primitives behind that pipeline.

use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result};
use image::DynamicImage;
use resvg::usvg::fontdb;

use crate::input::InputSource;

/// Lazily-initialized font database shared across all SVG parses. Loading
/// system fonts is slow (~100 ms — 1 s on macOS); we do it once. Without
/// it, SVGs containing `<text>` elements (notably termsvg / asciinema-svg
/// recordings) drop their glyphs entirely.
fn font_db() -> Arc<fontdb::Database> {
    static FONTDB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTDB
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

fn usvg_options() -> resvg::usvg::Options<'static> {
    resvg::usvg::Options {
        fontdb: font_db(),
        ..Default::default()
    }
}

/// Get the intrinsic dimensions of an SVG source.
pub fn svg_dimensions(source: &InputSource) -> Result<(u32, u32)> {
    let tree = load_svg(source)?;
    let size = tree.size();
    Ok((size.width().max(1.0) as u32, size.height().max(1.0) as u32))
}

/// Rasterize an SVG source to a bitmap at the given pixel dimensions.
pub fn rasterize_svg(source: &InputSource, width: u32, height: u32) -> Result<DynamicImage> {
    let tree = load_svg(source)?;
    rasterize_tree(&tree, width, height)
}

/// Rasterize an in-memory SVG document to a bitmap. Used by the animation
/// path, which generates a fresh SVG per frame and feeds it directly.
pub fn rasterize_svg_bytes(bytes: &[u8], width: u32, height: u32) -> Result<DynamicImage> {
    let tree =
        resvg::usvg::Tree::from_data(bytes, &usvg_options()).context("failed to parse SVG")?;
    rasterize_tree(&tree, width, height)
}

fn rasterize_tree(tree: &resvg::usvg::Tree, width: u32, height: u32) -> Result<DynamicImage> {
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).context("failed to create pixmap")?;

    let transform = resvg::tiny_skia::Transform::from_scale(
        width as f32 / tree.size().width(),
        height as f32 / tree.size().height(),
    );

    resvg::render(tree, transform, &mut pixmap.as_mut());

    // Convert premultiplied RGBA (tiny-skia) to straight RGBA (image crate)
    let data = pixmap.data();
    let mut rgba_buf = Vec::with_capacity((width * height * 4) as usize);
    for chunk in data.chunks(4) {
        let [pr, pg, pb, a] = [chunk[0], chunk[1], chunk[2], chunk[3]];
        if a == 0 {
            rgba_buf.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let af = a as f32 / 255.0;
            rgba_buf.push((pr as f32 / af).min(255.0) as u8);
            rgba_buf.push((pg as f32 / af).min(255.0) as u8);
            rgba_buf.push((pb as f32 / af).min(255.0) as u8);
            rgba_buf.push(a);
        }
    }

    let img = image::RgbaImage::from_raw(width, height, rgba_buf)
        .context("failed to create image from pixmap")?;
    Ok(DynamicImage::ImageRgba8(img))
}

fn load_svg(source: &InputSource) -> Result<resvg::usvg::Tree> {
    let svg_data = source.read_bytes().context("failed to read SVG")?;
    resvg::usvg::Tree::from_data(&svg_data, &usvg_options()).context("failed to parse SVG")
}
