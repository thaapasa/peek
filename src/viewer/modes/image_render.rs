use anyhow::Result;
use syntect::highlighting::Color;

use super::{Handled, Mode, ModeId, RenderCtx};
use crate::input::InputSource;
use crate::theme::PeekTheme;
use crate::viewer::image::{ImageConfig, render};
use crate::viewer::ui::Action;

#[derive(Copy, Clone)]
pub(crate) enum ImageKind {
    Raster,
    Svg,
}

/// Image content view: ASCII glyph rendering of a raster or rasterized
/// SVG. Owns the background mode; `b` cycles it. Re-renders on terminal
/// resize because the rendered glyph grid depends on terminal dimensions.
pub(crate) struct ImageRenderMode {
    source: InputSource,
    config: ImageConfig,
    kind: ImageKind,
    label: &'static str,
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

    fn render(&mut self, ctx: &RenderCtx) -> Result<Vec<String>> {
        let term = render::TermSize {
            cols: ctx.term_cols.min(u32::MAX as usize) as u32,
            rows: ctx.term_rows.min(u32::MAX as usize) as u32,
        };
        // ColorMode is interactive-cyclable, so read it from the live ctx
        // rather than the stale copy captured at construction time.
        self.config.color_mode = ctx.peek_theme.color_mode;
        match self.kind {
            ImageKind::Raster => render::load_and_render(&self.source, &self.config, term),
            ImageKind::Svg => render::load_and_render_svg(&self.source, &self.config, term),
        }
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
