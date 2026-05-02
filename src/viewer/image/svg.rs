use anyhow::{Context, Result};
use image::DynamicImage;

use crate::input::InputSource;
use crate::input::detect::FileType;
use crate::output::PrintOutput;

use super::Viewer;
use super::{ImageConfig, render};

/// Piped-output viewer for SVG sources. Rasterizes the SVG and runs the
/// resulting bitmap through the same ASCII-art pipeline as
/// [`super::ImageViewer`].
pub struct SvgViewer {
    config: ImageConfig,
}

impl SvgViewer {
    pub fn new(config: ImageConfig) -> Self {
        Self { config }
    }
}

impl Viewer for SvgViewer {
    fn render(
        &self,
        source: &InputSource,
        _file_type: &FileType,
        output: &mut PrintOutput,
    ) -> Result<()> {
        let term = render::TermSize::detect();
        let lines = render::load_and_render_svg(source, &self.config, term)?;
        for line in &lines {
            output.write_line(line)?;
        }
        Ok(())
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

    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).context("failed to create pixmap")?;

    let transform = resvg::tiny_skia::Transform::from_scale(
        width as f32 / tree.size().width(),
        height as f32 / tree.size().height(),
    );

    resvg::render(&tree, transform, &mut pixmap.as_mut());

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
    resvg::usvg::Tree::from_data(&svg_data, &resvg::usvg::Options::default())
        .context("failed to parse SVG")
}
