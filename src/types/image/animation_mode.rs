use std::time::Duration;

use anyhow::Result;
use syntect::highlighting::Color;

use super::anim_frame::AnimFrameState;
use super::pipeline::ImageConfig;
use super::pipeline::animate::AnimFrame;
use super::pipeline::render;
use super::scroll::ScrollBounds;
use super::view::ImageView;
use crate::theme::PeekTheme;
use crate::viewer::modes::{ExtractTarget, Handled, Mode, ModeId, RenderCtx, Window};
use crate::viewer::paged::{CYCLE_BACKGROUND_HELP, CYCLE_FIT_HELP, CYCLE_IMAGE_MODE_HELP};
use crate::viewer::ui::{Action, HelpEntry};

/// Animated image view (GIF / WebP). Owns the decoded frame list plus
/// shared frame-position / play state. Image-grid scroll, cycleable
/// config, and the render core live on the embedded [`ImageView`];
/// frame stepping + tick clock live on [`AnimFrameState`].
///
/// Each frame is independently prepare→composite→rendered — no
/// per-frame cache because the underlying `DynamicImage` changes every
/// tick. `ImageView`'s pan offsets persist across ticks (panning a long
/// banner GIF stays put while frames cycle).
///
/// **Not unified into an `AnimatedView<FrameSource>` shell with
/// [`crate::types::svg::animation_mode::SvgAnimationMode`] despite the
/// shared Mode-trait shape.** Cache strategies diverge by design: this
/// mode has no per-frame cache (decoded pixels already in memory; per
/// render = re-prepare); SVG holds a bounded LRU of rasterized frames.
/// A shared shell would either drop one of those optimisations or push
/// them through a fan-through enum that loses the type-level
/// distinction. Prior `/checkup` rounds decided the duplicated Mode
/// body (~80 lines) is cheaper than the abstraction.
pub(crate) struct AnimationMode {
    frames: Vec<AnimFrame>,
    anim: AnimFrameState,
    view: ImageView,
}

const ANIM_ACTIONS: &[HelpEntry] = &[
    (&[Action::PlayPause], "Play / pause"),
    (
        &[Action::NextFrame, Action::PrevFrame],
        "Next / previous frame",
    ),
    (&[Action::Extract], "Extract current frame as PNG"),
    CYCLE_BACKGROUND_HELP,
    CYCLE_IMAGE_MODE_HELP,
    CYCLE_FIT_HELP,
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Scroll left / right (FitHeight)",
    ),
];

impl AnimationMode {
    pub(crate) fn new(frames: Vec<AnimFrame>, config: ImageConfig) -> Self {
        assert!(!frames.is_empty(), "AnimationMode requires \u{2265}1 frame");
        Self {
            frames,
            anim: AnimFrameState::new(),
            view: ImageView::new(config),
        }
    }
}

impl Mode for AnimationMode {
    fn id(&self) -> ModeId {
        ModeId::Animation
    }

    fn label(&self) -> &str {
        "Animation"
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, _rows: usize) -> Result<Window> {
        let term = self.view.prepare_term(ctx);
        let frame = &self.frames[self.anim.current];
        let prep = render::prepare_decoded(frame.image.clone(), &self.view.config, term);
        Ok(self.view.render_prepared(&prep, term))
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
        // We don't keep the prepared grid bounds between calls (frames
        // change on every tick), so this handler just nudges the
        // offsets optimistically. The real clamp lives in
        // `render_window`, which computes max_scroll against the live
        // frame and pulls the saturated value back to the actual bound.
        self.view.scroll(action, ScrollBounds::unbounded())
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        ANIM_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        if let Some(h) = self.view.handle_config_cycle(action) {
            return h;
        }
        match action {
            Action::PlayPause => self.anim.play_pause(),
            Action::NextFrame => self.anim.step(self.frames.len(), true),
            Action::PrevFrame => self.anim.step(self.frames.len(), false),
            _ => Handled::No,
        }
    }

    fn next_tick(&self) -> Option<Duration> {
        self.anim.next_tick(self.frames[self.anim.current].delay)
    }

    fn tick(&mut self) -> bool {
        self.anim.tick(self.frames.len())
    }

    fn extract_target(&self) -> Option<ExtractTarget> {
        Some(self.anim.extract_target())
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let mut segs = self.view.status_segments(theme);
        segs.push((self.anim.status_segment(self.frames.len()), theme.label));
        segs
    }
}
