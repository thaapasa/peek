//! PDF page-render mode. One page at a time, rasterized via Pdfium and
//! ASCII-rendered through the shared image pipeline. Mirrors
//! [`crate::types::comic::cbz::CbzReadMode`] — same caching shape,
//! same n / p navigation, same image-config knobs.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::output::PrintOutput;
use crate::theme::{PeekTheme, StyleMode};
use crate::types::image::pipeline::ImageConfig;
use crate::types::image::pipeline::render::{
    self as image_render, GridWindow, TermSize, prepare_decoded,
};
use crate::viewer::cell_size::cell_aspect_h_over_w;
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window};
use crate::viewer::paged::{
    self, CachedRender, PageCacheKey, cycle_image_config, render_cached, step_paged,
};
use crate::viewer::ui::{Action, HelpEntry};

use super::package::Doc;

const EXTRA_ACTIONS: &[HelpEntry] = &[
    (
        &[Action::NextChapter, Action::PrevChapter],
        "Next / previous page",
    ),
    (
        &[Action::CycleBackground, Action::CycleBackgroundBack],
        "Cycle background",
    ),
    (
        &[Action::CycleImageMode, Action::CycleImageModeBack],
        "Cycle render mode",
    ),
    (
        &[Action::CycleFitMode],
        "Cycle fit (contain / width / height)",
    ),
];

pub(crate) struct PdfPageMode {
    doc: Doc,
    image_config: ImageConfig,
    page_count: usize,
    current: usize,
    cache: Vec<Option<CachedRender>>,
    warnings: Vec<String>,
}

impl PdfPageMode {
    pub(crate) fn new(doc: Doc, image_config: ImageConfig) -> Self {
        let page_count = doc.page_count();
        let mut cache = Vec::with_capacity(page_count);
        cache.resize_with(page_count, || None);
        // PDF pages are usually portrait + dense — fitting both axes
        // into the viewport (Contain) crushes a full A4 page into ~30
        // rows, illegible. FitWidth makes the page fill the terminal
        // width at correct aspect ratio; vertical scroll covers the
        // overflow. User can cycle back to Contain via `f` if they
        // want the whole-page-at-once view.
        let mut image_config = image_config;
        image_config.fit = crate::types::image::pipeline::FitMode::FitWidth;
        Self {
            doc,
            image_config,
            page_count,
            current: 0,
            cache,
            warnings: Vec::new(),
        }
    }

    fn ensure_rendered(
        &mut self,
        width: usize,
        rows: usize,
        style_mode: StyleMode,
    ) -> Result<&[String]> {
        if self.page_count == 0 {
            return Ok(&[]);
        }
        let idx = self.current;
        let key = PageCacheKey::build(&self.image_config, width, rows, style_mode);
        // Disjoint-borrow split: render closure captures `&self.doc`,
        // `self.image_config` (Copy), and `&mut self.warnings` while
        // `render_cached` holds `&mut self.cache`.
        let doc = &self.doc;
        let image_config = self.image_config;
        let warnings = &mut self.warnings;
        render_cached(&mut self.cache, idx, key, |k| {
            render_page(doc, image_config, idx, k, warnings)
        })
    }
}

fn render_page(
    doc: &Doc,
    image_config: ImageConfig,
    idx: usize,
    key: &PageCacheKey,
    warnings: &mut Vec<String>,
) -> Result<Vec<String>> {
    let mut config = image_config;
    config.style_mode = key.style_mode;
    let term = TermSize {
        cols: key.width as u32,
        rows: paged::pipe_rows(key.rows),
        cell_h_over_w: cell_aspect_h_over_w(),
    };
    // Rasterize at ~16 px per terminal column. Pdfium auto-scales
    // height to preserve native aspect ratio, so the downstream
    // image pipeline receives a correctly-proportioned bitmap and
    // FitWidth can size it to the terminal grid without squashing.
    let px_w = (key.width as u32 * 16).clamp(64, 4096);
    let img = match doc.render_page(idx, px_w) {
        Ok(i) => i,
        Err(e) => {
            warnings.push(format!("page {}: render failed: {e:#}", idx + 1));
            return Ok(vec![format!("[page {} render failed]", idx + 1)]);
        }
    };
    let prep = prepare_decoded(img, &config, term);
    let window = GridWindow::full(prep.cols, prep.rows);
    Ok(image_render::render_prepared(&prep, &config, window))
}

impl Mode for PdfPageMode {
    fn id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn label(&self) -> &str {
        "Read"
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let lines =
            self.ensure_rendered(ctx.term_cols, ctx.term_rows, ctx.peek_theme.style_mode)?;
        let total = lines.len();
        let win = slice_window(lines, scroll, rows);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache
            .get(self.current)
            .and_then(|c| c.as_ref())
            .map(|c| c.lines.len())
    }

    /// Print mode walks every page in order, separated by a blank
    /// line. Honors the cache so already-rendered pages reuse their
    /// output; interactive view stays single-page.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let total = self.page_count;
        let saved = self.current;
        for i in 0..total {
            self.current = i;
            let lines =
                self.ensure_rendered(ctx.term_cols, ctx.term_rows, ctx.peek_theme.style_mode)?;
            for line in lines {
                out.write_line(line)?;
            }
            if i + 1 < total {
                out.write_line("")?;
            }
        }
        self.current = saved;
        Ok(())
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        if let Some(h) = cycle_image_config(action, &mut self.image_config) {
            return h;
        }
        match action {
            Action::NextChapter => step_paged(&mut self.current, self.page_count, 1),
            Action::PrevChapter => step_paged(&mut self.current, self.page_count, -1),
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        if self.page_count == 0 {
            return Vec::new();
        }
        vec![(
            format!("page {}/{}", self.current + 1, self.page_count),
            theme.muted,
        )]
    }

    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> {
        if self.page_count <= 1 {
            return Vec::new();
        }
        vec!["n/p:page"]
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}
