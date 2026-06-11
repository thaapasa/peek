//! Shared single-bitmap paged rendering: the prepare → window-crop →
//! render dance every [`PageRenderer`](crate::viewer::paged::PageRenderer)
//! (PDF page, CBZ page, EPS preview / Ghostscript render) defers to.
//!
//! Lives here, beside the rasterization engine it drives, rather than in
//! the foundation `viewer::paged` module — it is only ever called from
//! the type-side page renderers, and it reaches into the `pipeline`
//! engine. Its data types (`PagedRender` / `RenderArgs`) come back from
//! the foundation, the allowed reader → foundation direction.

use super::pipeline::render::{self as image_render, GridWindow, prepare_decoded};
use crate::viewer::image_render::ImageConfig;
use crate::viewer::paged::{PagedRender, RenderArgs};

/// How the rendered viewport maps back to source-image pixels — the
/// clamped scroll origin plus the effective grid and the post-margin
/// source dimensions. A page-coordinate overlay (PDF text reconstruction)
/// projects through this: page units → source px → effective cells →
/// viewport cells. Mirrors the linear cell↔pixel projection in
/// [`crate::viewer::image_render::ZoomedView::pixel_roi`].
#[derive(Copy, Clone, Debug)]
pub(crate) struct GridMap {
    pub effective_cols: u32,
    pub effective_rows: u32,
    pub viewport_cols: u32,
    pub viewport_rows: u32,
    /// Scroll origin actually rendered (post-clamp), in effective cells.
    pub scroll_x: u32,
    pub scroll_y: u32,
    /// Post-margin source bitmap dimensions in pixels — the pixel space
    /// the effective grid projects onto.
    pub src_w: u32,
    pub src_h: u32,
}

/// Render the visible viewport of an already-decoded bitmap to ASCII
/// lines at the requested zoom / pan. Every single-bitmap `PageRenderer`
/// owns its own decode + cache, then defers this identical prepare →
/// window-crop → render dance here so the logic lives once.
pub(crate) fn render_image_window(
    img: &image::DynamicImage,
    config: ImageConfig,
    args: RenderArgs,
) -> PagedRender {
    render_image_window_mapped(img, config, args).0
}

/// [`render_image_window`] that also reports the viewport↔source
/// projection, for renderers that paint a page-coordinate overlay on
/// top of the rendered cells.
pub(crate) fn render_image_window_mapped(
    img: &image::DynamicImage,
    config: ImageConfig,
    args: RenderArgs,
) -> (PagedRender, GridMap) {
    let mut config = config;
    config.style_mode = args.style_mode;
    let prep = prepare_decoded(img.clone(), &config, args.term);
    let (src_w, src_h) = (prep.source.width(), prep.source.height());
    if args.zoom.is_one() {
        // Fast path: prepare_decoded already sized to the base grid;
        // crop the visible window for the current pan.
        let viewport_cols = prep.cols.min(args.term.cols).max(1);
        let viewport_rows = prep.rows.min(args.term.rows).max(1);
        let max_x = prep.cols.saturating_sub(viewport_cols);
        let max_y = prep.rows.saturating_sub(viewport_rows);
        let sx = args.scroll_x.min(max_x);
        let sy = args.scroll_y.min(max_y);
        let window = GridWindow {
            col_start: sx,
            col_end: sx + viewport_cols,
            row_start: sy,
            row_end: sy + viewport_rows,
        };
        let lines = image_render::render_prepared(&prep, &config, window);
        let render = PagedRender {
            lines,
            effective_cols: prep.cols,
            effective_rows: prep.rows,
            viewport_cols,
            viewport_rows,
        };
        let map = GridMap {
            effective_cols: prep.cols,
            effective_rows: prep.rows,
            viewport_cols,
            viewport_rows,
            scroll_x: sx,
            scroll_y: sy,
            src_w,
            src_h,
        };
        return (render, map);
    }
    // Reproduce the zoomed path's scroll clamp so the map reports the
    // origin actually rendered.
    let zv = crate::viewer::image_render::zoom::ZoomedView {
        base_cols: prep.cols,
        base_rows: prep.rows,
        term_cols: args.term.cols,
        term_rows: args.term.rows,
        zoom: args.zoom.factor(),
    };
    let (mut sx, mut sy) = (args.scroll_x, args.scroll_y);
    zv.clamp_scroll(&mut sx, &mut sy);
    let result = image_render::render_prepared_zoomed(
        &prep,
        &config,
        args.term,
        args.zoom.factor(),
        args.scroll_x,
        args.scroll_y,
    );
    let map = GridMap {
        effective_cols: result.effective_cols,
        effective_rows: result.effective_rows,
        viewport_cols: result.viewport_cols,
        viewport_rows: result.viewport_rows,
        scroll_x: sx,
        scroll_y: sy,
        src_w,
        src_h,
    };
    (
        PagedRender {
            lines: result.lines,
            effective_cols: result.effective_cols,
            effective_rows: result.effective_rows,
            viewport_cols: result.viewport_cols,
            viewport_rows: result.viewport_rows,
        },
        map,
    )
}
