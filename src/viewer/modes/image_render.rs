use anyhow::Result;
use syntect::highlighting::Color;

use super::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::input::InputSource;
use crate::theme::PeekTheme;
use crate::viewer::image::render::GridWindow;
use crate::viewer::image::{Background, FitMode, ImageConfig, ImageMode, render};
use crate::viewer::ui::Action;

#[derive(Copy, Clone)]
pub(crate) enum ImageKind {
    Raster,
    Svg,
}

/// Cache key for the post-decode/resize/composite intermediate. Mode
/// cycling between Ascii and the cell-grid modes changes target pixel
/// resolution, so the `ascii` flag is part of the key. The `fit` field
/// keeps the cache valid across `Contain` ↔ `FitWidth` ↔ `FitHeight`
/// toggles — each fit mode produces a different target grid and its own
/// composited intermediate.
#[derive(Copy, Clone, PartialEq, Eq)]
struct CacheKey {
    term_cols: u32,
    term_rows: u32,
    margin: u32,
    bg: Background,
    ascii: bool,
    fit: FitMode,
}

struct CachedFrame {
    key: CacheKey,
    prep: render::PreparedImage,
}

/// Image content view: ASCII glyph rendering of a raster or rasterized
/// SVG. Owns the background mode (`b` cycles it) and the fit mode (`f`
/// cycles it). Re-renders on terminal resize because the rendered glyph
/// grid depends on terminal dimensions.
///
/// Holds a single-slot cache of the decoded → resized → composited image.
/// Mode/color-mode cycling reuses the slot; terminal resize / margin /
/// background / fit-mode change miss and recompute, dropping the old
/// slot. Memory is bounded to one image at the current console setup.
///
/// In `FitWidth` / `FitHeight` the prepared grid may exceed the terminal
/// viewport on one axis; `scroll_x` / `scroll_y` track the offset into
/// the prepared grid. `Mode::owns_scroll() = true` so the global line
/// scroller doesn't fight us. Scroll is reset on fit-mode toggle (the
/// old offset has no meaning in the new grid).
pub(crate) struct ImageRenderMode {
    source: InputSource,
    config: ImageConfig,
    kind: ImageKind,
    label: &'static str,
    cache: Option<CachedFrame>,
    scroll_x: u32,
    scroll_y: u32,
}

const IMAGE_ACTIONS: &[(Action, &str)] = &[
    (Action::CycleBackground, "Cycle background (images)"),
    (Action::CycleImageMode, "Cycle render mode (images)"),
    (Action::CycleFitMode, "Cycle fit (contain / width / height)"),
    (Action::ScrollLeft, "Scroll left (FitHeight)"),
    (Action::ScrollRight, "Scroll right (FitHeight)"),
];

impl ImageRenderMode {
    pub(crate) fn new(source: InputSource, config: ImageConfig, kind: ImageKind) -> Self {
        let label = match kind {
            ImageKind::Raster => "Image",
            ImageKind::Svg => "Render",
        };
        Self {
            source,
            config,
            kind,
            label,
            cache: None,
            scroll_x: 0,
            scroll_y: 0,
        }
    }
}

impl Mode for ImageRenderMode {
    fn id(&self) -> ModeId {
        ModeId::ImageRender
    }

    fn label(&self) -> &str {
        self.label
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, _rows: usize) -> Result<Window> {
        let term = render::TermSize {
            cols: ctx.term_cols.min(u32::MAX as usize) as u32,
            rows: ctx.term_rows.min(u32::MAX as usize) as u32,
        };
        // ColorMode is interactive-cyclable, so read it from the live ctx
        // rather than the stale copy captured at construction time.
        self.config.color_mode = ctx.peek_theme.color_mode;

        let key = CacheKey {
            term_cols: term.cols,
            term_rows: term.rows,
            margin: self.config.margin,
            bg: self.config.background,
            ascii: matches!(self.config.mode, ImageMode::Ascii),
            fit: self.config.fit,
        };
        let needs_recompute = self.cache.as_ref().map(|c| c.key != key).unwrap_or(true);
        if needs_recompute {
            let prep = match self.kind {
                ImageKind::Raster => render::prepare_raster(&self.source, &self.config, term)?,
                ImageKind::Svg => render::prepare_svg(&self.source, &self.config, term)?,
            };
            self.cache = Some(CachedFrame { key, prep });
        }
        let prep = &self.cache.as_ref().unwrap().prep;

        // Clamp scroll to the current grid + viewport, then carve a window.
        // Visible viewport is min(term, prep) per axis — nothing past the
        // image edge is meaningful to render.
        let (max_x, max_y) = render::max_scroll(prep.cols, prep.rows, term.cols, term.rows);
        self.scroll_x = self.scroll_x.min(max_x);
        self.scroll_y = self.scroll_y.min(max_y);
        let visible_cols = prep.cols.min(term.cols);
        let visible_rows = prep.rows.min(term.rows);
        let window = GridWindow {
            col_start: self.scroll_x,
            col_end: self.scroll_x + visible_cols,
            row_start: self.scroll_y,
            row_end: self.scroll_y + visible_rows,
        };

        let lines = render::render_prepared(prep, &self.config, window);
        // `total` drives status-line position math elsewhere. Report the
        // full prepared row count so a scroll indicator (future) has the
        // right denominator; `Window.lines.len()` is the visible slice.
        let total = prep.rows as usize;
        Ok(Window { lines, total })
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    /// Pipe / `--print` path: row count is unbounded (`usize::MAX`), so
    /// `FitHeight` is meaningless and `FitWidth` reduces to `Contain`
    /// anyway. Force `Contain` regardless of the live config so that any
    /// future CLI flag for fit doesn't accidentally produce gigantic
    /// non-interactive output.
    fn render_to_pipe(
        &mut self,
        ctx: &RenderCtx,
        out: &mut crate::output::PrintOutput,
    ) -> Result<()> {
        let saved_fit = self.config.fit;
        let (saved_x, saved_y) = (self.scroll_x, self.scroll_y);
        self.config.fit = FitMode::Contain;
        self.scroll_x = 0;
        self.scroll_y = 0;
        self.cache = None;
        let window = self.render_window(ctx, 0, ctx.term_rows)?;
        for line in window.lines {
            out.write_line(&line)?;
        }
        self.config.fit = saved_fit;
        self.scroll_x = saved_x;
        self.scroll_y = saved_y;
        Ok(())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        // Compute current bounds from the cache. Without a cache the user
        // hasn't seen anything yet; ignore scroll until first render.
        let Some(cache) = &self.cache else {
            return false;
        };
        let (max_x, max_y) = render::max_scroll(
            cache.prep.cols,
            cache.prep.rows,
            cache.key.term_cols,
            cache.key.term_rows,
        );
        let page_y = cache.key.term_rows.saturating_sub(1).max(1);
        // Arrow-key horizontal step: small enough to feel responsive, big
        // enough to make progress across a wide image. Page-equivalent is
        // not bound on the horizontal axis (PgUp/PgDn stay vertical).
        const HSTEP: u32 = 4;
        match action {
            Action::ScrollUp => {
                self.scroll_y = self.scroll_y.saturating_sub(1);
                true
            }
            Action::ScrollDown => {
                self.scroll_y = (self.scroll_y + 1).min(max_y);
                true
            }
            Action::PageUp => {
                self.scroll_y = self.scroll_y.saturating_sub(page_y);
                true
            }
            Action::PageDown => {
                self.scroll_y = (self.scroll_y + page_y).min(max_y);
                true
            }
            Action::Top => {
                self.scroll_x = 0;
                self.scroll_y = 0;
                true
            }
            Action::Bottom => {
                self.scroll_y = max_y;
                true
            }
            Action::ScrollLeft => {
                self.scroll_x = self.scroll_x.saturating_sub(HSTEP);
                true
            }
            Action::ScrollRight => {
                self.scroll_x = (self.scroll_x + HSTEP).min(max_x);
                true
            }
            _ => false,
        }
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        IMAGE_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::CycleBackground => {
                self.config.background = self.config.background.next();
                Handled::Yes
            }
            Action::CycleImageMode => {
                self.config.mode = self.config.mode.next();
                Handled::Yes
            }
            Action::CycleFitMode => {
                self.config.fit = self.config.fit.next();
                self.scroll_x = 0;
                self.scroll_y = 0;
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        vec![
            (self.config.mode.label().to_string(), theme.label),
            (self.config.fit.label().to_string(), theme.label),
        ]
    }
}
