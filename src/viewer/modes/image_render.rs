use anyhow::Result;

use super::{Mode, ModeId, RenderCtx};
use crate::input::InputSource;
use crate::viewer::image::{Background, ImageConfig, render};
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
    background: Background,
    kind: ImageKind,
    label: &'static str,
}

const BG_CYCLE_ACTIONS: &[(Action, &str)] =
    &[(Action::CycleBackground, "Cycle background (images)")];

impl ImageRenderMode {
    pub(crate) fn new(source: InputSource, config: ImageConfig, kind: ImageKind) -> Self {
        let label = match kind {
            ImageKind::Raster => "Image",
            ImageKind::Svg => "Render",
        };
        Self {
            source,
            background: config.background,
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

    fn render(&mut self, _ctx: &RenderCtx) -> Result<Vec<String>> {
        let mut term = render::TermSize::detect();
        term.rows = term.rows.saturating_sub(1);
        let c = &self.config;
        match self.kind {
            ImageKind::Raster => render::load_and_render(
                &self.source,
                c.mode,
                c.width,
                term,
                self.background,
                c.margin,
            ),
            ImageKind::Svg => render::load_and_render_svg(
                &self.source,
                c.mode,
                c.width,
                term,
                self.background,
                c.margin,
            ),
        }
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        BG_CYCLE_ACTIONS
    }

    fn handle(&mut self, action: Action) -> bool {
        if action == Action::CycleBackground {
            self.background = self.background.next();
            true
        } else {
            false
        }
    }
}
