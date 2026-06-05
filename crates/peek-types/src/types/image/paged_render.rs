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

/// Render the visible viewport of an already-decoded bitmap to ASCII
/// lines at the requested zoom / pan. Every single-bitmap `PageRenderer`
/// owns its own decode + cache, then defers this identical prepare →
/// window-crop → render dance here so the logic lives once.
pub(crate) fn render_image_window(
    img: &image::DynamicImage,
    config: ImageConfig,
    args: RenderArgs,
) -> PagedRender {
    let mut config = config;
    config.style_mode = args.style_mode;
    let prep = prepare_decoded(img.clone(), &config, args.term);
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
        return PagedRender {
            lines,
            effective_cols: prep.cols,
            effective_rows: prep.rows,
            viewport_cols,
            viewport_rows,
        };
    }
    let result = image_render::render_prepared_zoomed(
        &prep,
        &config,
        args.term,
        args.zoom.factor(),
        args.scroll_x,
        args.scroll_y,
    );
    PagedRender {
        lines: result.lines,
        effective_cols: result.effective_cols,
        effective_rows: result.effective_rows,
        viewport_cols: result.viewport_cols,
        viewport_rows: result.viewport_rows,
    }
}
