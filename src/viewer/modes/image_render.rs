use anyhow::Result;
use syntect::highlighting::Color;

use super::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::input::InputSource;
use crate::theme::PeekTheme;
use crate::viewer::image::{Background, ImageConfig, ImageMode, render};
use crate::viewer::ui::Action;

#[derive(Copy, Clone)]
pub(crate) enum ImageKind {
    Raster,
    Svg,
}

/// Cache key for the post-decode/resize/composite intermediate. Mode
/// cycling between Ascii and the cell-grid modes changes target pixel
/// resolution, so the `ascii` flag is part of the key.
#[derive(Copy, Clone, PartialEq, Eq)]
struct CacheKey {
    term_cols: u32,
    term_rows: u32,
    margin: u32,
    bg: Background,
    ascii: bool,
}

struct CachedFrame {
    key: CacheKey,
    prep: render::PreparedImage,
}

/// Image content view: ASCII glyph rendering of a raster or rasterized
/// SVG. Owns the background mode; `b` cycles it. Re-renders on terminal
/// resize because the rendered glyph grid depends on terminal dimensions.
///
/// Holds a single-slot cache of the decoded → resized → composited image.
/// Mode/color-mode cycling reuses the slot; terminal resize / margin /
/// background change miss and recompute, dropping the old slot. Memory
/// is bounded to one image at the current console setup.
pub(crate) struct ImageRenderMode {
    source: InputSource,
    config: ImageConfig,
    kind: ImageKind,
    label: &'static str,
    cache: Option<CachedFrame>,
}

const IMAGE_ACTIONS: &[(Action, &str)] = &[
    (Action::CycleBackground, "Cycle background (images)"),
    (Action::CycleImageMode, "Cycle render mode (images)"),
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
        let lines = render::render_prepared(prep, &self.config);
        let total = lines.len();
        Ok(Window { lines, total })
    }

    fn rerender_on_resize(&self) -> bool {
        true
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
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        vec![(self.config.mode.label().to_string(), theme.label)]
    }
}
