use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};

use super::clustering::fast_2_color;
use super::glyph_atlas::{
    CELL_H, CELL_W, GlyphBitmap, atlas_for_mode, best_contour_glyph, best_glyph, dilate_bitmap,
};
use super::{Background, FitMode, ImageConfig, ImageMode};
use crate::input::InputSource;
use crate::theme::ColorMode;

/// Terminal dimensions in characters. The image renderer is fed sizes
/// from `RenderCtx` rather than querying the terminal itself, so the
/// same code path serves both interactive (live terminal size) and
/// pipe (`$COLUMNS or 80`, unbounded rows) rendering.
#[derive(Debug, Clone, Copy)]
pub struct TermSize {
    pub cols: u32,
    pub rows: u32,
}

/// Compute the rendered grid size `(cols, rows)` for an image. Aspect
/// ratio is always preserved; the `fit` argument decides which axis
/// constrains the result.
///
/// Aspect ratio rule for terminal cells (~2:1 height:width):
///   cols / (rows * 2) = img_w / img_h
///
/// - `forced_width > 0`: width is locked to that value, height follows
///   from aspect ratio. Ignores both `term` and `fit` — the CLI knob
///   `--width` is an explicit override.
/// - `Contain`: scale to fit entirely within `term`, constrained by
///   the smaller axis. Output never exceeds `term`.
/// - `FitWidth`: width = `term.cols`, height follows aspect ratio.
///   Output may exceed `term.rows` (vertical overflow → vertical scroll).
/// - `FitHeight`: height = `term.rows`, width follows aspect ratio.
///   Output may exceed `term.cols` (horizontal overflow → horizontal
///   scroll).
pub fn compute_grid(
    img_w: u32,
    img_h: u32,
    term: TermSize,
    forced_width: u32,
    fit: FitMode,
) -> (u32, u32) {
    if forced_width > 0 {
        let rows = (img_h as f64 * forced_width as f64 / (img_w as f64 * 2.0)) as u32;
        return (forced_width, rows.max(1));
    }
    match fit {
        FitMode::Contain => contain_grid(img_w, img_h, term),
        FitMode::FitWidth => {
            let rows = (img_h as f64 * term.cols as f64 / (img_w as f64 * 2.0)) as u32;
            (term.cols.max(1), rows.max(1))
        }
        FitMode::FitHeight => {
            let cols = (img_w as f64 * term.rows as f64 * 2.0 / img_h as f64) as u32;
            (cols.max(1), term.rows.max(1))
        }
    }
}

fn contain_grid(img_w: u32, img_h: u32, term: TermSize) -> (u32, u32) {
    let rows_from_width = (img_h as f64 * term.cols as f64 / (img_w as f64 * 2.0)) as u32;
    if rows_from_width <= term.rows {
        (term.cols, rows_from_width.max(1))
    } else {
        let cols_from_height = (img_w as f64 * term.rows as f64 * 2.0 / img_h as f64) as u32;
        (cols_from_height.clamp(1, term.cols), term.rows)
    }
}

/// A rectangular sub-grid of a `PreparedImage` to render.
///
/// Cell coordinates are in the full prepared grid (`PreparedImage::cols` /
/// `rows`). When the image fits the terminal entirely (`Contain`), this is
/// always the full grid; under `FitWidth` / `FitHeight` it carries the
/// scrolled, terminal-sized window into a larger grid.
#[derive(Debug, Clone, Copy)]
pub struct GridWindow {
    pub col_start: u32,
    pub col_end: u32,
    pub row_start: u32,
    pub row_end: u32,
}

impl GridWindow {
    #[allow(dead_code)]
    pub fn full(cols: u32, rows: u32) -> Self {
        Self {
            col_start: 0,
            col_end: cols,
            row_start: 0,
            row_end: rows,
        }
    }

    pub fn cols(&self) -> u32 {
        self.col_end.saturating_sub(self.col_start)
    }

    pub fn rows(&self) -> u32 {
        self.row_end.saturating_sub(self.row_start)
    }
}

/// Render an image using the block-color algorithm.
///
/// `full_cols` / `full_rows` describe the prepared grid (used to derive the
/// pixel canvas size). `window` selects which sub-rectangle of that grid is
/// emitted as lines — under fit modes that scroll, the renderer skips cells
/// outside the visible viewport instead of producing strings that would have
/// to be re-sliced (escape sequences make horizontal substring expensive).
///
/// Returns a vector of ANSI-colored lines, one per row in `window`.
pub fn render_block_color(
    img: &DynamicImage,
    full_cols: u32,
    full_rows: u32,
    window: GridWindow,
    mode: ImageMode,
    color_mode: ColorMode,
) -> Vec<String> {
    let plain = color_mode == ColorMode::Plain;
    let px_w = full_cols * CELL_W;
    let px_h = full_rows * CELL_H;
    let resized = if img.width() == px_w && img.height() == px_h {
        img.to_rgb8()
    } else {
        img.resize_exact(px_w, px_h, image::imageops::FilterType::Lanczos3)
            .to_rgb8()
    };

    let raw = resized.as_raw();
    let stride = (px_w * 3) as usize;

    let atlas_refs = atlas_for_mode(mode);
    let atlas: Vec<GlyphBitmap> = atlas_refs.iter().map(|g| **g).collect();

    let mut cell_pixels = [[0u8; 3]; 128];
    let mut lines = Vec::with_capacity(window.rows() as usize);

    for row in window.row_start..window.row_end {
        let mut line = String::with_capacity((window.cols() * 40) as usize);

        for col in window.col_start..window.col_end {
            let base_x = (col * CELL_W) as usize;
            let base_y = (row * CELL_H) as usize;

            for cy in 0..CELL_H as usize {
                for cx in 0..CELL_W as usize {
                    let px_offset = (base_y + cy) * stride + (base_x + cx) * 3;
                    cell_pixels[cy * CELL_W as usize + cx] =
                        [raw[px_offset], raw[px_offset + 1], raw[px_offset + 2]];
                }
            }

            let (ch, fg, bg) = if plain {
                let (bits, shade) = mono_cell(&cell_pixels);
                let ch = shade.unwrap_or_else(|| best_glyph(bits, &atlas).ch);
                (ch, [0; 3], [0; 3])
            } else {
                let cluster = fast_2_color(&cell_pixels);
                let glyph_match = best_glyph(cluster.bitmap, &atlas);
                let (fg, bg) = if glyph_match.inverted {
                    (cluster.color_b, cluster.color_a)
                } else {
                    (cluster.color_a, cluster.color_b)
                };
                (glyph_match.ch, fg, bg)
            };

            color_mode.write_fg_bg(&mut line, fg, bg, ch);
        }

        line.push_str(color_mode.reset());
        lines.push(line);
    }

    lines
}

/// Variance threshold below which a Plain-mode cell is treated as uniform
/// and rendered with a shade-ramp glyph instead of a spatial bitmap match.
/// ~400 ≈ 20 luma stddev.
const UNIFORM_VAR_THRESHOLD: f32 = 400.0;

/// Plain-mode cell mapping. Returns either:
/// - `(bitmap, None)` — bit i set if pixel i is at or above cell mean luma;
///   caller runs through `best_glyph` for spatial pattern.
/// - `(0, Some(ch))` — low-variance cell; glyph picked from a 5-step shade
///   ramp by mean luma. Avoids degenerate empty/full glyphs for flat regions.
///
/// Polarity convention: ink represents brighter pixels (suits the common
/// light-on-dark terminal default).
fn mono_cell(px: &[[u8; 3]; 128]) -> (u128, Option<char>) {
    let lumas: [f32; 128] = std::array::from_fn(|i| {
        let [r, g, b] = px[i];
        0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
    });
    let mean = lumas.iter().sum::<f32>() / 128.0;
    let variance = lumas.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / 128.0;

    if variance < UNIFORM_VAR_THRESHOLD {
        const RAMP: [char; 5] = [' ', '░', '▒', '▓', '█'];
        let idx = ((mean / 255.0) * 4.0).round() as usize;
        return (0, Some(RAMP[idx.min(4)]));
    }

    let mut bits: u128 = 0;
    for (i, l) in lumas.iter().enumerate() {
        if *l >= mean {
            bits |= 1u128 << i;
        }
    }
    (bits, None)
}

/// Render a binary edge image as line-art glyphs.
///
/// `edges` is the output of [`super::contour::detect_edges`] — pure white
/// pixels on pure black. Per cell we build a bitmap (bit = 1 where pixel
/// is an edge) and ask the existing glyph matcher for the best line shape.
///
/// Colors are emitted as foreground only — edge pixels render in the
/// theme's bright fg, the void uses terminal default bg. This avoids the
/// 2-cluster algorithm's polarity ambiguity on sparse-edge cells and lets
/// the result blend with whatever terminal theme the user has.
pub fn render_contour(
    edges: &DynamicImage,
    full_cols: u32,
    full_rows: u32,
    window: GridWindow,
    mode: ImageMode,
    color_mode: ColorMode,
) -> Vec<String> {
    let px_w = full_cols * CELL_W;
    let px_h = full_rows * CELL_H;
    let resized = if edges.width() == px_w && edges.height() == px_h {
        edges.to_rgb8()
    } else {
        edges
            .resize_exact(px_w, px_h, image::imageops::FilterType::Nearest)
            .to_rgb8()
    };

    let raw = resized.as_raw();
    let stride = (px_w * 3) as usize;

    let atlas_refs = atlas_for_mode(mode);
    let atlas: Vec<GlyphBitmap> = atlas_refs.iter().map(|g| **g).collect();
    let dilated_atlas: Vec<u128> = atlas.iter().map(|g| dilate_bitmap(g.bits)).collect();

    let edge_fg: [u8; 3] = [230, 230, 230];
    let mut lines = Vec::with_capacity(window.rows() as usize);

    for row in window.row_start..window.row_end {
        let mut line = String::with_capacity((window.cols() * 20) as usize);

        for col in window.col_start..window.col_end {
            let base_x = (col * CELL_W) as usize;
            let base_y = (row * CELL_H) as usize;

            let mut bits: u128 = 0;
            for cy in 0..CELL_H as usize {
                for cx in 0..CELL_W as usize {
                    let off = (base_y + cy) * stride + (base_x + cx) * 3;
                    if raw[off] >= 128 {
                        bits |= 1u128 << (cy * CELL_W as usize + cx);
                    }
                }
            }

            if bits == 0 {
                line.push(' ');
                continue;
            }
            let ch = best_contour_glyph(bits, &atlas, &dilated_atlas);
            color_mode.write_fg(&mut line, edge_fg, ch);
        }

        line.push_str(color_mode.reset());
        lines.push(line);
    }

    lines
}

/// Render an image using the legacy density-ramp algorithm.
/// Returns a vector of ANSI-colored lines.
pub fn render_density(
    img: &DynamicImage,
    full_cols: u32,
    full_rows: u32,
    window: GridWindow,
    color_mode: ColorMode,
) -> Vec<String> {
    const DENSITY_RAMP: &[u8] =
        b" .'`^\",:;Il!i><~+_-?][}{1)(|/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$";

    let resized = if img.width() == full_cols && img.height() == full_rows {
        img.clone()
    } else {
        img.resize_exact(full_cols, full_rows, image::imageops::FilterType::Lanczos3)
    };

    let ramp_len = DENSITY_RAMP.len();
    let mut lines = Vec::with_capacity(window.rows() as usize);

    for y in window.row_start..window.row_end {
        let mut line = String::with_capacity((window.cols() * 20) as usize);
        for x in window.col_start..window.col_end {
            let pixel = resized.get_pixel(x, y);
            let [r, g, b, _a] = pixel.0;

            let luma = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
            let idx = ((luma / 255.0) * (ramp_len - 1) as f64) as usize;
            let ch = DENSITY_RAMP[idx.min(ramp_len - 1)] as char;

            color_mode.write_fg(&mut line, [r, g, b], ch);
        }
        line.push_str(color_mode.reset());
        lines.push(line);
    }

    lines
}

/// Add transparent margin around an image.
pub fn add_margin(img: DynamicImage, margin: u32) -> DynamicImage {
    if margin == 0 {
        return img;
    }
    let (w, h) = img.dimensions();
    // Canvas is initialized to [0,0,0,0] (fully transparent).
    let mut canvas = image::RgbaImage::new(w + margin * 2, h + margin * 2);
    image::imageops::overlay(&mut canvas, &img.to_rgba8(), margin as i64, margin as i64);
    DynamicImage::ImageRgba8(canvas)
}

/// Check if an image has an alpha channel.
fn has_alpha(img: &DynamicImage) -> bool {
    use image::ColorType;
    matches!(
        img.color(),
        ColorType::Rgba8
            | ColorType::Rgba16
            | ColorType::Rgba32F
            | ColorType::La8
            | ColorType::La16
    )
}

/// Analyze non-transparent pixels to choose a compositing background.
/// Dark content → white background, light content → black background.
fn auto_background(img: &DynamicImage) -> [u8; 3] {
    let rgba = img.to_rgba8();
    let (mut luma_sum, mut count) = (0.0f64, 0u64);
    for pixel in rgba.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 10 {
            continue;
        }
        luma_sum += 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
        count += 1;
    }
    if count == 0 {
        return [255, 255, 255];
    }
    if luma_sum / (count as f64) < 128.0 {
        [255, 255, 255]
    } else {
        [0, 0, 0]
    }
}

/// Resolve a Background setting to an RGB color for a given pixel position.
fn resolve_bg(bg: Background, img: &DynamicImage) -> Box<dyn Fn(u32, u32) -> [u8; 3]> {
    match bg {
        Background::Auto => {
            let color = if has_alpha(img) {
                auto_background(img)
            } else {
                [0, 0, 0]
            };
            Box::new(move |_x, _y| color)
        }
        Background::Black => Box::new(|_x, _y| [0, 0, 0]),
        Background::White => Box::new(|_x, _y| [255, 255, 255]),
        Background::Checkerboard => {
            // Half-block-sized checkerboard (8x8 px = one half-block glyph)
            Box::new(|x, y| {
                let cell = (x / 8 + y / 8) % 2;
                if cell == 0 {
                    [204, 204, 204]
                } else {
                    [102, 102, 102]
                }
            })
        }
    }
}

/// Composite an RGBA image against a background, returning an RGB image.
fn composite_onto(img: &DynamicImage, bg_fn: &dyn Fn(u32, u32) -> [u8; 3]) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut rgb = image::RgbImage::new(w, h);
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = a as f32 / 255.0;
        let inv = 1.0 - alpha;
        let bg = bg_fn(x, y);
        rgb.put_pixel(
            x,
            y,
            image::Rgb([
                (r as f32 * alpha + bg[0] as f32 * inv) as u8,
                (g as f32 * alpha + bg[1] as f32 * inv) as u8,
                (b as f32 * alpha + bg[2] as f32 * inv) as u8,
            ]),
        );
    }
    DynamicImage::ImageRgb8(rgb)
}

/// Apply alpha compositing with the given background mode.
pub fn composite_with_bg(img: DynamicImage, bg: Background) -> DynamicImage {
    if !has_alpha(&img) && bg == Background::Auto {
        return img;
    }
    let bg_fn = resolve_bg(bg, &img);
    composite_onto(&img, &*bg_fn)
}

/// Output of the decode → margin → resize → composite pipeline; the
/// mode-specific glyph render runs against this. Cached by
/// `ImageRenderMode` so mode/color-mode cycling skips the costly
/// decode + Lanczos + composite stages.
pub struct PreparedImage {
    pub composited: DynamicImage,
    pub cols: u32,
    pub rows: u32,
}

/// Run the load → margin → resize → composite pipeline for a raster source.
pub fn prepare_raster(
    source: &InputSource,
    config: &ImageConfig,
    term: TermSize,
) -> Result<PreparedImage> {
    Ok(prepare_decoded(load_image(source)?, config, term))
}

/// Run margin → resize → composite on an already-decoded image.
pub fn prepare_decoded(img: DynamicImage, config: &ImageConfig, term: TermSize) -> PreparedImage {
    let img = add_margin(img, config.margin);
    let (img_w, img_h) = img.dimensions();
    let (cols, rows) = compute_grid(img_w, img_h, term, config.width, config.fit);

    let (px_w, px_h) = match config.mode {
        ImageMode::Ascii => (cols, rows),
        _ => (cols * CELL_W, rows * CELL_H),
    };
    let img = img.resize_exact(px_w, px_h, image::imageops::FilterType::Lanczos3);
    let img = composite_with_bg(img, config.background);

    PreparedImage {
        composited: img,
        cols,
        rows,
    }
}

/// Mode-specific glyph render against an already-prepared image. `window`
/// selects the visible sub-rectangle of the prepared grid; pass
/// `GridWindow::full(prep.cols, prep.rows)` for a full render.
pub fn render_prepared(
    prep: &PreparedImage,
    config: &ImageConfig,
    window: GridWindow,
) -> Vec<String> {
    match config.mode {
        ImageMode::Ascii => render_density(
            &prep.composited,
            prep.cols,
            prep.rows,
            window,
            config.color_mode,
        ),
        ImageMode::Contour => {
            let edges = super::contour::detect_edges(&prep.composited, config.edge_density);
            render_contour(
                &edges,
                prep.cols,
                prep.rows,
                window,
                config.mode,
                config.color_mode,
            )
        }
        ImageMode::Full | ImageMode::Block | ImageMode::Geo => render_block_color(
            &prep.composited,
            prep.cols,
            prep.rows,
            window,
            config.mode,
            config.color_mode,
        ),
    }
}

/// Load an image from a File path or buffered Stdin bytes.
pub fn load_image(source: &InputSource) -> Result<DynamicImage> {
    match source {
        InputSource::File(path) => image::open(path).context("failed to open image"),
        InputSource::Stdin { data } => {
            image::load_from_memory(data).context("failed to decode image from stdin")
        }
    }
}

/// Run the rasterize → margin → composite pipeline for an SVG source.
pub fn prepare_svg(
    source: &InputSource,
    config: &ImageConfig,
    term: TermSize,
) -> Result<PreparedImage> {
    let (svg_w, svg_h) = super::svg::svg_dimensions(source)?;
    let margin = config.margin;
    // Account for margin in aspect ratio calculation
    let padded_w = svg_w + margin * 2;
    let padded_h = svg_h + margin * 2;
    let (cols, rows) = compute_grid(padded_w, padded_h, term, config.width, config.fit);

    // Compute target pixel size, then rasterize SVG into the inner area
    // (target minus margin) so that adding margin reaches exact target size.
    let (px_w, px_h) = match config.mode {
        ImageMode::Ascii => (cols, rows),
        _ => (cols * CELL_W, rows * CELL_H),
    };
    let scale_x = px_w as f64 / padded_w as f64;
    let scale_y = px_h as f64 / padded_h as f64;
    let target_margin_x = (margin as f64 * scale_x).round() as u32;
    let target_margin_y = (margin as f64 * scale_y).round() as u32;
    let inner_w = px_w.saturating_sub(target_margin_x * 2).max(1);
    let inner_h = px_h.saturating_sub(target_margin_y * 2).max(1);

    let inner = super::svg::rasterize_svg(source, inner_w, inner_h)?;
    // Place the SVG content centered in a full-size transparent canvas
    let mut canvas = image::RgbaImage::new(px_w, px_h);
    let offset_x = (px_w - inner_w) / 2;
    let offset_y = (px_h - inner_h) / 2;
    image::imageops::overlay(
        &mut canvas,
        &inner.to_rgba8(),
        offset_x as i64,
        offset_y as i64,
    );
    let img = DynamicImage::ImageRgba8(canvas);
    let img = composite_with_bg(img, config.background);

    Ok(PreparedImage {
        composited: img,
        cols,
        rows,
    })
}
