//! PDF page renderer: rasterizes one page via Pdfium and ASCII-renders
//! it through the shared image pipeline. Plugged into the generic
//! [`crate::viewer::paged::PagedImageMode`].
//!
//! Still on the naive zoom path: Pdfium rasterises the full effective
//! grid (`term × zoom`) into a per-page cache, and each render slices
//! the visible viewport out of it. Memory grows with `zoom²`. The
//! per-page cache lives here so pan stays a cheap slice instead of
//! re-driving Pdfium; switching to Pdfium's clip-rect render API for
//! true ROI rasterisation is tracked in `docs/planned.md` under "Zoom
//! in PDF / font specimen — ROI-only render".

use std::cell::RefCell;

use anyhow::Result;

use crate::types::image::pipeline::ImageConfig;
use crate::types::image::pipeline::render::{
    self as image_render, GridWindow, TermSize, prepare_decoded,
};
use crate::viewer::paged::{self, PageCacheKey, PageRenderer, PagedRender, RenderArgs};
use crate::viewer::ui::{slice_styled_h, strip_ansi_width};

use super::package::Doc;

pub(crate) struct PdfPageRenderer {
    doc: Doc,
    /// Per-page effective-grid render cache. Holds the full zoom-aware
    /// ASCII grid keyed by `(scaled_width, scaled_rows, style, mode,
    /// background, fit)` so revisiting a page at the same zoom + image
    /// config skips re-rasterisation. Pan calls slice the visible
    /// window out of the cached grid; zoom or fit changes evict by
    /// failing the key match on next render.
    cache: RefCell<Vec<Option<CachedGrid>>>,
}

struct CachedGrid {
    key: PageCacheKey,
    lines: Vec<String>,
    cols: u32,
    rows: u32,
}

impl PdfPageRenderer {
    pub(crate) fn new(doc: Doc) -> Self {
        let count = doc.page_count();
        let mut cache = Vec::with_capacity(count);
        cache.resize_with(count, || None);
        Self {
            doc,
            cache: RefCell::new(cache),
        }
    }
}

impl PageRenderer for PdfPageRenderer {
    fn page_count(&self) -> usize {
        self.doc.page_count()
    }

    fn render_page(
        &self,
        idx: usize,
        config: ImageConfig,
        args: RenderArgs,
        warnings: &mut Vec<String>,
    ) -> Result<PagedRender> {
        let zoom_factor = args.zoom.factor();
        let scaled_cols = ((args.term.cols as f32 * zoom_factor).round() as u32).max(1);
        let scaled_rows = if args.term.rows == 0 {
            1
        } else {
            ((args.term.rows as f32 * zoom_factor).round() as u32).max(1)
        };
        let scaled_term = TermSize {
            cols: scaled_cols,
            rows: scaled_rows,
            cell_h_over_w: args.term.cell_h_over_w,
        };
        let mut config = config;
        config.style_mode = args.style_mode;
        let key = PageCacheKey::build(
            &config,
            scaled_cols as usize,
            scaled_rows as usize,
            args.style_mode,
        );
        self.ensure_grid_cached(idx, key, &config, scaled_term, warnings)?;

        // Slice the visible viewport out of the cached effective grid.
        let cache = self.cache.borrow();
        let cached = cache
            .get(idx)
            .and_then(|c| c.as_ref())
            .expect("ensure_grid_cached just populated the slot");
        let eff_cols = cached.cols;
        let eff_rows = cached.rows;
        let viewport_cols = eff_cols.min(args.term.cols).max(1);
        let viewport_rows = eff_rows
            .min(paged::pipe_rows(args.term.rows as usize))
            .max(1);
        let max_x = eff_cols.saturating_sub(viewport_cols);
        let max_y = eff_rows.saturating_sub(viewport_rows);
        let sx = args.scroll_x.min(max_x);
        let sy = args.scroll_y.min(max_y);
        let lines = slice_grid(
            &cached.lines,
            sx,
            sy,
            viewport_cols,
            viewport_rows,
            eff_cols,
        );
        Ok(PagedRender {
            lines,
            effective_cols: eff_cols,
            effective_rows: eff_rows,
            viewport_cols,
            viewport_rows,
        })
    }
}

impl PdfPageRenderer {
    fn ensure_grid_cached(
        &self,
        idx: usize,
        key: PageCacheKey,
        config: &ImageConfig,
        scaled_term: TermSize,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        let stale = self
            .cache
            .borrow()
            .get(idx)
            .and_then(|c| c.as_ref())
            .is_none_or(|c| c.key != key);
        if !stale {
            return Ok(());
        }
        // Rasterize at ~16 px per terminal column (capped). Pdfium
        // auto-scales height to preserve native aspect ratio so the
        // downstream pipeline can size the cell grid without squashing.
        let px_w = (scaled_term.cols * 16).clamp(64, 4096);
        let img = match self.doc.render_page(idx, px_w) {
            Ok(i) => i,
            Err(e) => {
                warnings.push(format!("page {}: render failed: {e:#}", idx + 1));
                let placeholder = format!("[page {} render failed]", idx + 1);
                let cols = placeholder.len() as u32;
                let mut cache = self.cache.borrow_mut();
                cache[idx] = Some(CachedGrid {
                    key,
                    lines: vec![placeholder],
                    cols,
                    rows: 1,
                });
                return Ok(());
            }
        };
        let prep = prepare_decoded(img, config, scaled_term);
        let window = GridWindow::full(prep.cols, prep.rows);
        let lines = image_render::render_prepared(&prep, config, window);
        let mut cache = self.cache.borrow_mut();
        cache[idx] = Some(CachedGrid {
            key,
            lines,
            cols: prep.cols,
            rows: prep.rows,
        });
        Ok(())
    }
}

/// Slice a `viewport_cols × viewport_rows` window starting at
/// `(scroll_x, scroll_y)` out of a cached effective-grid render.
/// Skips the SGR-aware horizontal slice when no horizontal pan is
/// needed.
fn slice_grid(
    lines: &[String],
    scroll_x: u32,
    scroll_y: u32,
    viewport_cols: u32,
    viewport_rows: u32,
    eff_cols: u32,
) -> Vec<String> {
    let start = scroll_y as usize;
    let end = (start + viewport_rows as usize).min(lines.len());
    let vert = &lines[start..end];
    if scroll_x == 0 && eff_cols <= viewport_cols {
        vert.iter()
            .map(|l| {
                if strip_ansi_width(l) as u32 <= viewport_cols {
                    l.clone()
                } else {
                    slice_styled_h(l, 0, viewport_cols as usize)
                }
            })
            .collect()
    } else {
        vert.iter()
            .map(|l| slice_styled_h(l, scroll_x as usize, viewport_cols as usize))
            .collect()
    }
}
