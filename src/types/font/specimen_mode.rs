//! Font specimen view: rasterise a hard-coded sample sentence through
//! the font, then route the resulting image through the standard ASCII
//! pipeline. Reuses [`ImageView`] for the cycleable image-config +
//! pan state, mirroring the structure of `ImageRenderMode` — the only
//! divergent piece is the source: a pre-decoded `DynamicImage` produced
//! once at construction from the font bytes, rather than a `&InputSource`
//! decoded per resize.

use anyhow::Result;
use image::DynamicImage;
use syntect::highlighting::Color;

use crate::theme::PeekTheme;
use crate::types::image::pipeline::render::{self, PreparedImage, TermSize};
use crate::types::image::pipeline::{Background, FitMode, ImageConfig, ImageMode};
use crate::types::image::scroll::ScrollBounds;
use crate::types::image::view::ImageView;
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::viewer::paged::{CYCLE_BACKGROUND_HELP, CYCLE_FIT_HELP, CYCLE_IMAGE_MODE_HELP};
use crate::viewer::ui::{Action, HelpEntry};

/// Cache key for the prepared specimen — same shape as
/// `ImageRenderMode::CacheKey` so the cycle behaviour (mode-cycle keeps
/// the slot live across colour-mode changes; resize / fit / background
/// / margin invalidate) matches the static image view.
#[derive(Copy, Clone, PartialEq, Eq)]
struct CacheKey {
    term_cols: u32,
    term_rows: u32,
    margin: u32,
    bg: Background,
    ascii: bool,
    fit: FitMode,
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
    prep: PreparedImage,
}

const SPECIMEN_ACTIONS: &[HelpEntry] = &[
    CYCLE_BACKGROUND_HELP,
    CYCLE_IMAGE_MODE_HELP,
    CYCLE_FIT_HELP,
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Scroll left / right (FitHeight)",
    ),
];

pub(crate) struct SpecimenMode {
    /// Pre-rasterised RGBA8 sample. Built once when the mode is
    /// composed; image-config changes route through `prepare_decoded`
    /// against this same buffer (the cached `composited` is the
    /// resize+composite result that varies with terminal size).
    decoded: DynamicImage,
    view: ImageView,
    cache: Option<CachedFrame>,
}

impl SpecimenMode {
    pub(crate) fn new(decoded: DynamicImage, config: ImageConfig) -> Self {
        Self {
            decoded,
            view: ImageView::new(config),
            cache: None,
        }
    }

    fn ensure_prepared(&mut self, key: CacheKey, term: TermSize) {
        let stale = self.cache.as_ref().map(|c| c.key != key).unwrap_or(true);
        if stale {
            let prep = render::prepare_decoded(self.decoded.clone(), &self.view.config, term);
            self.cache = Some(CachedFrame { key, prep });
        }
    }
}

impl Mode for SpecimenMode {
    fn id(&self) -> ModeId {
        ModeId::ImageRender
    }

    fn label(&self) -> &str {
        "Specimen"
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, _rows: usize) -> Result<Window> {
        let term = self.view.prepare_term(ctx);
        let key = CacheKey::build(&self.view.config, term);
        self.ensure_prepared(key, term);
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
        // Forced-Contain + scroll=0 produces a different grid than the
        // interactive cache slot — drop and let render_window rebuild.
        self.cache = None;
        let capped = ctx.capped_for_image_pipe();
        let window = self.render_window(&capped, 0, capped.term_rows)?;
        ImageView::write_lines(out, window)?;
        self.view.restore(snap);
        Ok(())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
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
        SPECIMEN_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        self.view.handle_config_cycle(action).unwrap_or(Handled::No)
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        self.view.status_segments(theme)
    }
}
