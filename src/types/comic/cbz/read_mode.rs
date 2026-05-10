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
use crate::types::image::pipeline::render::{
    self as image_render, GridWindow, TermSize, prepare_decoded,
};
use crate::types::image::pipeline::{Background, FitMode, ImageConfig, ImageMode};
use crate::viewer::cell_size::cell_aspect_h_over_w;
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window};
use crate::viewer::ui::Action;

use super::package::{self, Page};

const EXTRA_ACTIONS: &[(Action, &str)] = &[
    (Action::NextChapter, "Next page"),
    (Action::PrevChapter, "Previous page"),
    (Action::CycleBackground, "Cycle background"),
    (Action::CycleBackgroundBack, "Cycle background backward"),
    (Action::CycleImageMode, "Cycle render mode"),
    (Action::CycleImageModeBack, "Cycle render mode backward"),
    (Action::CycleFitMode, "Cycle fit (contain / width / height)"),
];

/// Cap on inline image height in pipe / `--print` mode where
/// `term_rows` is unbounded; otherwise a single page would
/// dominate the output.
const PIPE_IMAGE_MAX_ROWS: u32 = 30;

pub(crate) struct CbzReadMode {
    source: InputSource,
    image_config: ImageConfig,
    pages: Vec<Page>,
    current: usize,
    cache: Vec<Option<CachedPage>>,
    warnings: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    width: usize,
    rows: usize,
    style_mode: StyleMode,
    image_mode: ImageMode,
    background: Background,
    fit: FitMode,
}

struct CachedPage {
    key: CacheKey,
    lines: Vec<String>,
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

    fn key_for(&self, width: usize, rows: usize, style_mode: StyleMode) -> CacheKey {
        CacheKey {
            width,
            rows,
            style_mode,
            image_mode: self.image_config.mode,
            background: self.image_config.background,
            fit: self.image_config.fit,
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
        let key = self.key_for(width, rows, style_mode);
        let needs = self
            .cache
            .get(idx)
            .and_then(|c| c.as_ref())
            .map(|c| c.key != key)
            .unwrap_or(true);
        if needs {
            let lines = self.render_page(idx, &key)?;
            self.cache[idx] = Some(CachedPage { key, lines });
        }
        Ok(&self
            .cache
            .get(idx)
            .and_then(|c| c.as_ref())
            .expect("cache populated")
            .lines)
    }

    fn render_page(&mut self, idx: usize, key: &CacheKey) -> Result<Vec<String>> {
        let page = self.pages[idx].clone();
        let mut zip = match package::open_zip(&self.source) {
            Ok(z) => z,
            Err(e) => {
                self.warnings.push(format!("page {}: {e:#}", idx + 1));
                return Ok(vec![format!("[page {} unavailable]", idx + 1)]);
            }
        };
        let bytes = match package::read_page(&mut zip, &page.full_path) {
            Ok(b) => b,
            Err(e) => {
                self.warnings.push(format!("page {}: {e:#}", idx + 1));
                return Ok(vec![format!("[page {} unavailable]", idx + 1)]);
            }
        };
        let img = match image::load_from_memory(&bytes) {
            Ok(i) => i,
            Err(e) => {
                self.warnings
                    .push(format!("page {}: decode failed: {e:#}", idx + 1));
                return Ok(vec![format!("[page {} decode failed]", idx + 1)]);
            }
        };
        let mut config = self.image_config;
        config.style_mode = key.style_mode;
        let rows = if key.rows == usize::MAX {
            PIPE_IMAGE_MAX_ROWS
        } else {
            key.rows.min(u32::MAX as usize) as u32
        };
        let term = TermSize {
            cols: key.width as u32,
            rows,
            cell_h_over_w: cell_aspect_h_over_w(),
        };
        let prep = prepare_decoded(img, &config, term);
        let window = GridWindow::full(prep.cols, prep.rows);
        Ok(image_render::render_prepared(&prep, &config, window))
    }
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

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::NextChapter => {
                if self.pages.is_empty() {
                    return Handled::No;
                }
                let next = (self.current + 1).min(self.pages.len() - 1);
                if next == self.current {
                    return Handled::Yes;
                }
                self.current = next;
                Handled::YesResetScroll
            }
            Action::PrevChapter => {
                if self.current == 0 {
                    return Handled::Yes;
                }
                self.current = self.current.saturating_sub(1);
                Handled::YesResetScroll
            }
            Action::CycleBackground => {
                self.image_config.background = self.image_config.background.next();
                Handled::Yes
            }
            Action::CycleBackgroundBack => {
                self.image_config.background = self.image_config.background.prev();
                Handled::Yes
            }
            Action::CycleImageMode => {
                self.image_config.mode = self.image_config.mode.next();
                Handled::Yes
            }
            Action::CycleImageModeBack => {
                self.image_config.mode = self.image_config.mode.prev();
                Handled::Yes
            }
            Action::CycleFitMode => {
                self.image_config.fit = self.image_config.fit.next();
                Handled::Yes
            }
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
        vec!["n/N:page"]
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}
