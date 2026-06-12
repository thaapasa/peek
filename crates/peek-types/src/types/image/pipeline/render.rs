use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};

use super::clustering::fast_2_color;
use super::glyph_atlas::{
    CELL_H, CELL_W, GlyphBitmap, atlas_for_mode, best_contour_glyph, best_glyph, dilate_bitmap,
};
use super::{Background, FitMode, ImageConfig, ImageMode};
use crate::input::InputSource;
use crate::theme::StyleMode;
// TermSize moved to the foundation with the rest of the render vocab;
// re-exported so engine-internal `render::TermSize` paths are unchanged.
pub use crate::viewer::image_render::TermSize;

/// Compute the rendered grid size `(cols, rows)` for an image. Aspect
/// ratio is always preserved; the `fit` argument decides which axis
/// constrains the result.
///
/// Aspect ratio rule:
///   `cols / (rows * cell_h_over_w) = img_w / img_h`
/// where `cell_h_over_w` is the terminal's actual cell aspect ratio
/// (cell height ÷ cell width). The conventional 2.0 means "cell twice
/// as tall as wide" — fonts that don't match drag the rendered image
/// off-aspect, which is what the override exists to fix.
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
    let aspect = term.cell_h_over_w.max(0.1);
    if forced_width > 0 {
        // `--width` is the user's own axis — honor it past the fit cap
        // instead of folding it into `clamp_grid`, which would quietly
        // shrink an explicit `--width 2000` to 1024. Only the *derived*
        // rows axis is file-controlled (aspect-ratio metadata), so only
        // it gets the [`MAX_FIT_CELLS`] defence; an extreme aspect
        // squashes rather than overriding the requested width. The
        // width itself is capped at [`MAX_FORCED_WIDTH_CELLS`] only as
        // a typo / OOM guard.
        let cols = forced_width.min(MAX_FORCED_WIDTH_CELLS);
        let rows = (img_h as f64 * cols as f64 / (img_w as f64 * aspect)) as u32;
        return (cols, rows.clamp(1, MAX_FIT_CELLS));
    }
    let (cols, rows) = match fit {
        FitMode::Contain => contain_grid(img_w, img_h, term, aspect),
        FitMode::FitWidth => {
            let rows = (img_h as f64 * term.cols as f64 / (img_w as f64 * aspect)) as u32;
            (term.cols.max(1), rows.max(1))
        }
        FitMode::FitHeight => {
            let cols = (img_w as f64 * term.rows as f64 * aspect / img_h as f64) as u32;
            (cols.max(1), term.rows.max(1))
        }
    };
    clamp_grid(cols, rows)
}

/// Sanity ceiling on an explicit `--width`, in cells. Far above any real
/// terminal or pipe consumer, low enough that the cell→pixel arithmetic
/// stays in `u32` range when a typo'd width meets a tall image. Distinct
/// from [`MAX_FIT_CELLS`], which defends the file-controlled derived
/// axis — the user's explicit axis is honored well past the fit cap.
///
/// Memory worst case is deliberately bigger than the fit path's: at
/// this ceiling with the rows axis at [`MAX_FIT_CELLS`], the pixel
/// buffer is `2048·8 × 1024·16 × 4 B` ≈ 1 GiB — roughly 7× the fit
/// path's ~150 MB. Capping the cols×rows *area* instead would squash a
/// legitimate square image at `--width 2048` (true aspect needs the
/// full 1024 rows), and shrinking cols would override the explicit
/// width this path exists to honor. The 1 GiB case is opt-in: it takes
/// an explicit near-ceiling `--width`, never file content alone.
const MAX_FORCED_WIDTH_CELLS: u32 = 2048;

/// Ceiling on a grid axis, in cells. `FitWidth` / `FitHeight` /
/// `--width` derive one axis from the image's aspect ratio — metadata
/// the file controls — and the downstream pixel buffers scale with the
/// grid (`cells × CELL_W/H × 4` bytes), so an extreme aspect would
/// otherwise size them into gigabytes. 1024 cells is ~10 terminal
/// heights of scroll; the worst buffer stays ~150 MB at a 300-col
/// terminal. An explicit `--width` is *not* subject to this cap (see
/// `compute_grid`'s forced path) — only its derived rows axis is.
const MAX_FIT_CELLS: u32 = 1024;

/// Clamp a grid into the [`MAX_FIT_CELLS`] box, preserving the cell
/// aspect ratio — an over-ceiling fit degrades to containment inside
/// the capped box (proportionate, scrollable, bounded) rather than a
/// squashed render.
fn clamp_grid(cols: u32, rows: u32) -> (u32, u32) {
    let (mut cols, mut rows) = (cols.max(1), rows.max(1));
    if rows > MAX_FIT_CELLS {
        cols = ((cols as f64 * MAX_FIT_CELLS as f64 / rows as f64) as u32).max(1);
        rows = MAX_FIT_CELLS;
    }
    if cols > MAX_FIT_CELLS {
        rows = ((rows as f64 * MAX_FIT_CELLS as f64 / cols as f64) as u32).max(1);
        cols = MAX_FIT_CELLS;
    }
    (cols, rows)
}

fn contain_grid(img_w: u32, img_h: u32, term: TermSize, aspect: f64) -> (u32, u32) {
    let rows_from_width = (img_h as f64 * term.cols as f64 / (img_w as f64 * aspect)) as u32;
    if rows_from_width <= term.rows {
        (term.cols, rows_from_width.max(1))
    } else {
        let cols_from_height = (img_w as f64 * term.rows as f64 * aspect / img_h as f64) as u32;
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

/// Maximum scroll offset on each axis given the prepared grid size and
/// the visible viewport. Returns `(max_x, max_y)`; an axis with no
/// overflow returns 0.
pub fn max_scroll(prep_cols: u32, prep_rows: u32, term_cols: u32, term_rows: u32) -> (u32, u32) {
    (
        prep_cols.saturating_sub(term_cols),
        prep_rows.saturating_sub(term_rows),
    )
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
    style_mode: StyleMode,
) -> Vec<String> {
    let plain = style_mode == StyleMode::Plain;
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

            style_mode.write_fg_bg(&mut line, fg, bg, ch);
        }

        line.push_str(style_mode.reset());
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
    style_mode: StyleMode,
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
            style_mode.write_fg(&mut line, edge_fg, ch);
        }

        line.push_str(style_mode.reset());
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
    style_mode: StyleMode,
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

            style_mode.write_fg(&mut line, [r, g, b], ch);
        }
        line.push_str(style_mode.reset());
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
///
/// `composited` is the resized + composited image at the *base*
/// (zoom = 1) cell grid — the fast path for `ImageView::render_prepared`
/// when zoom = 1. `source` is the post-margin native-resolution image
/// *before* alpha compositing; the zoom > 1 path crops the matching
/// pixel ROI from it, then composites only that crop. Keeping `source`
/// pre-composite means animation prep stays cheap — compositing a
/// full-res alpha frame every tick would be the dominant cost otherwise.
pub struct PreparedImage {
    pub composited: DynamicImage,
    pub source: DynamicImage,
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
    let resized = img.resize_exact(px_w, px_h, image::imageops::FilterType::Lanczos3);
    let composited = composite_with_bg(resized, config.background);

    PreparedImage {
        composited,
        source: img,
        cols,
        rows,
    }
}

/// Output of the zoom > 1 render path. The lines are the rendered
/// viewport; the dimensions describe the effective grid (base ×
/// zoom) so the caller can clamp scroll and feed the status line.
pub struct ZoomedRender {
    pub lines: Vec<String>,
    pub effective_cols: u32,
    pub effective_rows: u32,
    pub viewport_cols: u32,
    pub viewport_rows: u32,
    /// Scroll origin actually rendered (post-clamp), in effective
    /// cells. Overlay painters project through this — reporting it
    /// here keeps the clamp in one place.
    pub scroll_x: u32,
    pub scroll_y: u32,
}

/// Render the visible viewport of an image at zoom > 1 by cropping the
/// native-resolution source to the matching pixel ROI and rescaling
/// only that crop. Memory peaks at one viewport-sized intermediate;
/// growing the zoom level does not enlarge the working buffer.
///
/// `scroll_x` / `scroll_y` are cell offsets in the *effective* grid
/// (= base × zoom). They are clamped here to keep the viewport on the
/// effective grid, and the clamped values are reflected back through
/// the returned `viewport_*` dimensions.
pub fn render_prepared_zoomed(
    prep: &PreparedImage,
    config: &ImageConfig,
    term: TermSize,
    zoom: f32,
    scroll_x: u32,
    scroll_y: u32,
) -> ZoomedRender {
    let zv = super::super::zoom::ZoomedView {
        base_cols: prep.cols,
        base_rows: prep.rows,
        term_cols: term.cols,
        term_rows: term.rows,
        zoom,
    };
    let (effective_cols, effective_rows) = zv.effective();
    let (viewport_cols, viewport_rows) = zv.viewport();
    let mut scroll_x = scroll_x;
    let mut scroll_y = scroll_y;
    zv.clamp_scroll(&mut scroll_x, &mut scroll_y);

    let roi = zv.pixel_roi(
        prep.source.width(),
        prep.source.height(),
        scroll_x,
        scroll_y,
    );
    let crop = prep.source.crop_imm(roi.x, roi.y, roi.w, roi.h);

    let full_window = GridWindow::full(viewport_cols, viewport_rows);
    let resize_to = |w: u32, h: u32| -> DynamicImage {
        let resized = crop.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
        // Source is held pre-composite so animation per-tick prep stays
        // cheap; composite the small viewport-sized crop here.
        composite_with_bg(resized, config.background)
    };
    let lines = match config.mode {
        ImageMode::Ascii => {
            let target = resize_to(viewport_cols, viewport_rows);
            render_density(
                &target,
                viewport_cols,
                viewport_rows,
                full_window,
                config.style_mode,
            )
        }
        ImageMode::Contour => {
            let target = resize_to(viewport_cols * CELL_W, viewport_rows * CELL_H);
            let edges = super::contour::detect_edges(&target, config.edge_density);
            render_contour(
                &edges,
                viewport_cols,
                viewport_rows,
                full_window,
                config.mode,
                config.style_mode,
            )
        }
        ImageMode::Full | ImageMode::Block | ImageMode::Geo => {
            let target = resize_to(viewport_cols * CELL_W, viewport_rows * CELL_H);
            render_block_color(
                &target,
                viewport_cols,
                viewport_rows,
                full_window,
                config.mode,
                config.style_mode,
            )
        }
    };

    ZoomedRender {
        lines,
        effective_cols,
        effective_rows,
        viewport_cols,
        viewport_rows,
        scroll_x,
        scroll_y,
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
            config.style_mode,
        ),
        ImageMode::Contour => {
            let edges = super::contour::detect_edges(&prep.composited, config.edge_density);
            render_contour(
                &edges,
                prep.cols,
                prep.rows,
                window,
                config.mode,
                config.style_mode,
            )
        }
        ImageMode::Full | ImageMode::Block | ImageMode::Geo => render_block_color(
            &prep.composited,
            prep.cols,
            prep.rows,
            window,
            config.mode,
            config.style_mode,
        ),
    }
}

/// Load an image from a file path or in-memory / ranged byte source.
/// Uses magic-byte format detection rather than file extension so a
/// misnamed file (e.g. PNG renamed to `.svg`) still decodes correctly
/// when the detection layer routes it to the image viewer.
///
/// Decode allocation is bounded by the `image` crate's default
/// `Limits` (512 MiB max alloc — both paths construct an
/// `ImageReader`, which applies them), so a header claiming absurd
/// dimensions fails cleanly instead of alloc-aborting. Deliberately
/// above the in-house caps: a legitimate 100-megapixel photo should
/// still open.
pub fn load_image(source: &InputSource) -> Result<DynamicImage> {
    match source {
        InputSource::File(path) => image::ImageReader::open(path)
            .context("failed to open image")?
            .with_guessed_format()
            .context("failed to guess image format")?
            .decode()
            .context("failed to decode image"),
        _ => {
            let buf = source.read_bytes()?;
            image::load_from_memory(&buf).context("failed to decode image")
        }
    }
}

/// Ceiling on the SVG rasterise target per axis — the analogue of the
/// PDF renderer's `PDFIUM_RENDER_CAP_PX`.
const SVG_RASTER_CAP_PX: u32 = 4096;

/// `base × bucket`, capped at [`SVG_RASTER_CAP_PX`] — but never below
/// `base` itself, so a grid that already exceeds the cap at zoom 1
/// still renders at its base size.
fn cap_raster_axis(base: u32, bucket: u32) -> u32 {
    base.saturating_mul(bucket)
        .clamp(1, base.max(SVG_RASTER_CAP_PX))
}

/// Run the rasterize → margin → composite pipeline for an SVG source.
/// `zoom_bucket` (≥ 1) multiplies the rasterise target so the
/// `source` field carries enough pixel detail to ROI-crop sharply at
/// the live zoom level. The cell grid (`cols` / `rows`) stays at base
/// regardless of bucket — only the source bitmap grows.
pub fn prepare_svg(
    source: &InputSource,
    config: &ImageConfig,
    term: TermSize,
    zoom_bucket: u32,
) -> Result<PreparedImage> {
    let (svg_w, svg_h) = super::svg::svg_dimensions(source)?;
    prepare_svg_inner(
        |w, h| super::svg::rasterize_svg(source, w, h),
        svg_w,
        svg_h,
        config,
        term,
        zoom_bucket,
    )
}

/// Like [`prepare_svg`] but rasterizes from in-memory SVG bytes — used by
/// the SVG animation pipeline, which mints a fresh document per frame.
/// The caller supplies the SVG's intrinsic pixel dimensions (typically
/// computed once at decode time via `svg::svg_dimensions`).
pub fn prepare_svg_bytes(
    bytes: &[u8],
    svg_w: u32,
    svg_h: u32,
    config: &ImageConfig,
    term: TermSize,
    zoom_bucket: u32,
) -> Result<PreparedImage> {
    prepare_svg_inner(
        |w, h| super::svg::rasterize_svg_bytes(bytes, w, h),
        svg_w,
        svg_h,
        config,
        term,
        zoom_bucket,
    )
}

fn prepare_svg_inner(
    rasterize: impl FnOnce(u32, u32) -> Result<DynamicImage>,
    svg_w: u32,
    svg_h: u32,
    config: &ImageConfig,
    term: TermSize,
    zoom_bucket: u32,
) -> Result<PreparedImage> {
    let margin = config.margin;
    let padded_w = svg_w + margin * 2;
    let padded_h = svg_h + margin * 2;
    let (cols, rows) = compute_grid(padded_w, padded_h, term, config.width, config.fit);

    let bucket = zoom_bucket.max(1);
    let (base_px_w, base_px_h) = match config.mode {
        ImageMode::Ascii => (cols, rows),
        _ => (cols * CELL_W, rows * CELL_H),
    };
    // Rasterise the source bitmap at `bucket × base` so the ROI crop
    // at zoom > 1 reads native detail rather than upscaling pixels —
    // but cap each axis (pdfium's 4096 px ceiling, same idea): a large
    // terminal grid times a deep zoom bucket would otherwise reach
    // multi-GB pixmaps. Past the cap zoom still works, the crop just
    // upscales instead of re-rasterising sharper.
    let px_w = cap_raster_axis(base_px_w, bucket);
    let px_h = cap_raster_axis(base_px_h, bucket);
    let scale_x = px_w as f64 / padded_w as f64;
    let scale_y = px_h as f64 / padded_h as f64;
    let target_margin_x = (margin as f64 * scale_x).round() as u32;
    let target_margin_y = (margin as f64 * scale_y).round() as u32;
    let inner_w = px_w.saturating_sub(target_margin_x * 2).max(1);
    let inner_h = px_h.saturating_sub(target_margin_y * 2).max(1);

    let inner = rasterize(inner_w, inner_h)?;
    let mut canvas = image::RgbaImage::new(px_w, px_h);
    let offset_x = (px_w - inner_w) / 2;
    let offset_y = (px_h - inner_h) / 2;
    image::imageops::overlay(
        &mut canvas,
        &inner.to_rgba8(),
        offset_x as i64,
        offset_y as i64,
    );
    let pre_composite = DynamicImage::ImageRgba8(canvas);

    // `composited` drives the zoom = 1 fast path so it must stay at
    // base dims. When the source bitmap is bucket-upscaled, resize it
    // down for the composite. Skip the resize at bucket = 1 — the
    // source already matches the base grid.
    let composited_pre = if bucket == 1 {
        pre_composite.clone()
    } else {
        let resized =
            pre_composite.resize_exact(base_px_w, base_px_h, image::imageops::FilterType::Lanczos3);
        DynamicImage::ImageRgba8(resized.to_rgba8())
    };
    let composited = composite_with_bg(composited_pre, config.background);

    Ok(PreparedImage {
        source: pre_composite,
        composited,
        cols,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::cell_size;

    #[test]
    fn extreme_aspect_grid_is_clamped_and_proportionate() {
        let term = cell_size::term_size(300, 100);
        // 1 px wide, 10M px tall at FitWidth: unclamped rows would be in
        // the millions; the clamp contains the grid in the capped box.
        let (cols, rows) = compute_grid(1, 10_000_000, term, 0, FitMode::FitWidth);
        assert!(rows <= MAX_FIT_CELLS, "rows {rows}");
        assert!((1..=300).contains(&cols), "cols {cols}");
        // 10M px wide, 1 px tall at FitHeight: same on the other axis.
        let (cols, rows) = compute_grid(10_000_000, 1, term, 0, FitMode::FitHeight);
        assert!(cols <= MAX_FIT_CELLS, "cols {cols}");
        assert!(rows >= 1, "rows {rows}");
        // --width: the derived rows axis is clamped, the explicit width
        // is honored as given (the file's aspect can't shrink it).
        let (cols, rows) = compute_grid(1, 10_000_000, term, 80, FitMode::Contain);
        assert_eq!(cols, 80, "explicit width survives an extreme aspect");
        assert!(rows <= MAX_FIT_CELLS, "forced-width rows {rows}");
    }

    /// An explicit `--width` above [`MAX_FIT_CELLS`] must be honored,
    /// not silently folded into the fit clamp — `--width 2000` emits
    /// 2000 columns. Only the typo-guard ceiling bounds it.
    #[test]
    fn forced_width_is_honored_past_the_fit_cap() {
        let term = cell_size::term_size(120, 40);
        // Wide image so derived rows stay small: width is the user's call.
        let (cols, rows) = compute_grid(4000, 100, term, 2000, FitMode::Contain);
        assert_eq!(cols, 2000);
        assert!((1..=MAX_FIT_CELLS).contains(&rows), "rows {rows}");
        // The sanity ceiling still applies.
        let (cols, _) = compute_grid(4000, 100, term, 1_000_000, FitMode::Contain);
        assert_eq!(cols, MAX_FORCED_WIDTH_CELLS);
    }

    #[test]
    fn normal_grids_pass_through_unclamped() {
        let term = cell_size::term_size(120, 40);
        let (cols, rows) = compute_grid(800, 600, term, 0, FitMode::Contain);
        assert!(cols <= 120 && rows <= 40, "{cols}x{rows}");
        let (cols, rows) = compute_grid(800, 1600, term, 0, FitMode::FitWidth);
        assert_eq!(cols, 120);
        assert!(rows > 40 && rows <= MAX_FIT_CELLS, "rows {rows}");
    }

    #[test]
    fn raster_axis_caps_zoom_target_but_never_below_base() {
        // Base under the cap: bucket multiplies until the cap.
        assert_eq!(cap_raster_axis(640, 1), 640);
        assert_eq!(cap_raster_axis(640, 4), 2560);
        assert_eq!(cap_raster_axis(640, 100), SVG_RASTER_CAP_PX);
        // Base already over the cap (tall clamped grid): stays at base.
        assert_eq!(cap_raster_axis(8000, 1), 8000);
        assert_eq!(cap_raster_axis(8000, 16), 8000);
    }
}
