//! CBZ read mode: one page at a time.
//!
//! Renders the page at `current` through the image pipeline (decode
//! once, ASCII-art per (cols, rows)). `n` / `N` step forward / back
//! through the page list, resetting the scroll offset for the new
//! page. The render cache is keyed by `(page, cols, rows, style,
//! image config)` so resizing or cycling color / fit re-renders only
//! the visible page and stepping back reuses prior renders.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::input::InputSource;
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

use super::package::{self, Page};

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

pub(crate) struct CbzReadMode {
    source: InputSource,
    image_config: ImageConfig,
    pages: Vec<Page>,
    current: usize,
    cache: Vec<Option<CachedRender>>,
    warnings: Vec<String>,
}

impl CbzReadMode {
    pub(crate) fn new(source: InputSource, image_config: ImageConfig, pages: Vec<Page>) -> Self {
        let n = pages.len();
        let mut cache = Vec::with_capacity(n);
        cache.resize_with(n, || None);
        Self {
            source,
            image_config,
            pages,
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
        if self.pages.is_empty() {
            return Ok(&[]);
        }
        let idx = self.current;
        let key = PageCacheKey::build(&self.image_config, width, rows, style_mode);
        // Disjoint-borrow split: render closure captures `&self.source`,
        // `&self.pages`, `self.image_config` (Copy), and
        // `&mut self.warnings` while `render_cached` holds
        // `&mut self.cache`.
        let source = &self.source;
        let pages = &self.pages;
        let image_config = self.image_config;
        let warnings = &mut self.warnings;
        render_cached(&mut self.cache, idx, key, |k| {
            render_page(source, pages, image_config, idx, k, warnings)
        })
    }
}

fn render_page(
    source: &InputSource,
    pages: &[Page],
    image_config: ImageConfig,
    idx: usize,
    key: &PageCacheKey,
    warnings: &mut Vec<String>,
) -> Result<Vec<String>> {
    let page = pages[idx].clone();
    let mut zip = match package::open_zip(source) {
        Ok(z) => z,
        Err(e) => {
            warnings.push(format!("page {}: {e:#}", idx + 1));
            return Ok(vec![format!("[page {} unavailable]", idx + 1)]);
        }
    };
    let bytes = match package::read_page(&mut zip, &page.full_path) {
        Ok(b) => b,
        Err(e) => {
            warnings.push(format!("page {}: {e:#}", idx + 1));
            return Ok(vec![format!("[page {} unavailable]", idx + 1)]);
        }
    };
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i,
        Err(e) => {
            warnings.push(format!("page {}: decode failed: {e:#}", idx + 1));
            return Ok(vec![format!("[page {} decode failed]", idx + 1)]);
        }
    };
    let mut config = image_config;
    config.style_mode = key.style_mode;
    let term = TermSize {
        cols: key.width as u32,
        rows: paged::pipe_rows(key.rows),
        cell_h_over_w: cell_aspect_h_over_w(),
    };
    let prep = prepare_decoded(img, &config, term);
    let window = GridWindow::full(prep.cols, prep.rows);
    Ok(image_render::render_prepared(&prep, &config, window))
}

impl Mode for CbzReadMode {
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
    /// output. Interactive view stays single-page; only the pipe
    /// path materializes the whole book.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let total = self.pages.len();
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
            Action::NextChapter => step_paged(&mut self.current, self.pages.len(), 1),
            Action::PrevChapter => step_paged(&mut self.current, self.pages.len(), -1),
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        if self.pages.is_empty() {
            return Vec::new();
        }
        vec![(
            format!("page {}/{}", self.current + 1, self.pages.len()),
            theme.muted,
        )]
    }

    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> {
        if self.pages.len() <= 1 {
            return Vec::new();
        }
        vec!["n/p:page"]
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}
