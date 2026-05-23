use anyhow::Result;
use syntect::highlighting::Color;

use super::pipeline::render::{self, TermSize};
use super::pipeline::{ImageConfig, ImageMode};
use super::scroll::ScrollBounds;
use super::view::ImageView;
use crate::input::InputSource;
use crate::theme::PeekTheme;
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::viewer::paged::{CYCLE_BACKGROUND_HELP, CYCLE_FIT_HELP, CYCLE_IMAGE_MODE_HELP};
use crate::viewer::ui::{Action, HelpEntry};

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
/// composited intermediate. `cell_h_over_w` is omitted intentionally:
/// it's cached per-process on first read and never changes mid-session.
#[derive(Copy, Clone, PartialEq, Eq)]
struct CacheKey {
    term_cols: u32,
    term_rows: u32,
    margin: u32,
    bg: super::pipeline::Background,
    ascii: bool,
    fit: super::pipeline::FitMode,
}

impl CacheKey {
    fn build(config: &ImageConfig, term: TermSize) -> Self {
        Self {
            term_cols: term.cols,
            term_rows: term.rows,
            margin: config.margin,
            bg: config.background,
            ascii: matches!(config.mode, ImageMode::Ascii),
            fit: config.fit,
        }
    }
}

struct CachedFrame {
    key: CacheKey,
    prep: render::PreparedImage,
}

/// Image content view: ASCII glyph rendering of a raster or rasterized
/// SVG. Image-grid scroll + cycleable config live on the embedded
/// [`ImageView`]; this Mode owns the source plus a single-slot cache of
/// the decoded → resized → composited intermediate. Mode/color-mode
/// cycling reuses the slot; terminal resize / margin / background /
/// fit-mode change miss and recompute, dropping the old slot. Memory is
/// bounded to one image at the current console setup.
pub(crate) struct ImageRenderMode {
    source: InputSource,
    kind: ImageKind,
    label: &'static str,
    view: ImageView,
    cache: Option<CachedFrame>,
}

const IMAGE_ACTIONS: &[HelpEntry] = &[
    CYCLE_BACKGROUND_HELP,
    CYCLE_IMAGE_MODE_HELP,
    CYCLE_FIT_HELP,
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Scroll left / right (FitHeight)",
    ),
];

impl ImageRenderMode {
    pub(crate) fn new(source: InputSource, config: ImageConfig, kind: ImageKind) -> Self {
        let label = match kind {
            ImageKind::Raster => "Image",
            ImageKind::Svg => "Render",
        };
        Self::with_label(source, config, kind, label)
    }

    /// Like [`Self::new`] but with a caller-supplied label — used when
    /// the same render machinery drives a more specific view (e.g.
    /// the audio "Cover" tab).
    pub(crate) fn with_label(
        source: InputSource,
        config: ImageConfig,
        kind: ImageKind,
        label: &'static str,
    ) -> Self {
        Self {
            source,
            kind,
            label,
            view: ImageView::new(config),
            cache: None,
        }
    }

    /// Repopulate `cache` if stale for the given key. After this the
    /// cached `prep` is always live for `key`.
    fn ensure_prepared(&mut self, key: CacheKey, term: TermSize) -> Result<()> {
        let stale = self.cache.as_ref().map(|c| c.key != key).unwrap_or(true);
        if stale {
            let prep = match self.kind {
                ImageKind::Raster => render::prepare_raster(&self.source, &self.view.config, term)?,
                ImageKind::Svg => render::prepare_svg(&self.source, &self.view.config, term)?,
            };
            self.cache = Some(CachedFrame { key, prep });
        }
        Ok(())
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
        let term = self.view.prepare_term(ctx);
        let key = CacheKey::build(&self.view.config, term);
        self.ensure_prepared(key, term)?;
        let prep = &self.cache.as_ref().expect("populated above").prep;
        Ok(self.view.render_prepared(prep, term))
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_to_pipe(
        &mut self,
        ctx: &RenderCtx,
        out: &mut crate::output::PrintOutput,
    ) -> Result<()> {
        let snap = self.view.pipe_snapshot();
        // Drop the cached intermediate — the forced Contain + scroll=0
        // produces a different grid than the interactive cache slot.
        self.cache = None;
        let window = self.render_window(ctx, 0, ctx.term_rows)?;
        ImageView::write_lines(out, window)?;
        self.view.restore(snap);
        Ok(())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        // Without a cache the user hasn't seen anything yet — ignore scroll.
        let Some(cache) = &self.cache else {
            return false;
        };
        let (max_x, max_y) = render::max_scroll(
            cache.prep.cols,
            cache.prep.rows,
            cache.key.term_cols,
            cache.key.term_rows,
        );
        let page_y = cache.key.term_rows.saturating_sub(1);
        self.view
            .scroll(action, ScrollBounds::clamped(max_x, max_y, page_y))
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        IMAGE_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        self.view.handle_config_cycle(action).unwrap_or(Handled::No)
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        self.view.status_segments(theme)
    }
}
