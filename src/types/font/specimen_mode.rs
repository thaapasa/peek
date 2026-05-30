//! Font specimen view: rasterise a hard-coded sample sentence through
//! the font, then route the resulting image through the standard ASCII
//! pipeline. Reuses [`ImageView`] for the cycleable image-config +
//! pan state, mirroring the structure of `ImageRenderMode` — the only
//! divergent piece is the source: a pre-decoded `DynamicImage` produced
//! once at construction from the font bytes, rather than a `&InputSource`
//! decoded per resize.

use anyhow::Result;
use bytes::Bytes;
use image::DynamicImage;
use syntect::highlighting::Color;

use crate::theme::PeekTheme;
use crate::types::font::specimen;
use crate::types::image::pipeline::render::{self, PreparedImage, TermSize};
use crate::types::image::pipeline::{Background, FitMode, ImageConfig, ImageMode};
use crate::types::image::scroll::ScrollBounds;
use crate::types::image::view::ImageView;
use crate::types::image::zoom::integer_bucket;
use crate::viewer::cell_size;
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

const ZOOM_HELP: &[HelpEntry] = &[
    (&[Action::ZoomIn, Action::ZoomOut], "Zoom in / out"),
    (&[Action::ZoomReset], "Reset zoom to 1×"),
    (
        &[
            Action::ZoomPreset(1),
            Action::ZoomPreset(2),
            Action::ZoomPreset(3),
            Action::ZoomPreset(4),
            Action::ZoomPreset(5),
            Action::ZoomPreset(6),
            Action::ZoomPreset(7),
            Action::ZoomPreset(8),
            Action::ZoomPreset(9),
        ],
        "Zoom 1×–9×",
    ),
];

const SPECIMEN_ACTIONS: &[HelpEntry] = &[
    CYCLE_BACKGROUND_HELP,
    CYCLE_IMAGE_MODE_HELP,
    CYCLE_FIT_HELP,
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Scroll left / right (FitHeight)",
    ),
    ZOOM_HELP[0],
    ZOOM_HELP[1],
    ZOOM_HELP[2],
];

const SPECIMEN_ACTIONS_WITH_FACE_CYCLE: &[HelpEntry] = &[
    CYCLE_BACKGROUND_HELP,
    CYCLE_IMAGE_MODE_HELP,
    CYCLE_FIT_HELP,
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Scroll left / right (FitHeight)",
    ),
    (
        &[Action::NextFace, Action::PrevFace],
        "Next / previous face",
    ),
    ZOOM_HELP[0],
    ZOOM_HELP[1],
    ZOOM_HELP[2],
];

pub(crate) struct SpecimenMode {
    /// Original font bytes — kept so face-cycle keys can re-rasterise a
    /// different face without re-reading the source. Cheap to clone
    /// since [`Bytes`] is refcounted.
    bytes: Bytes,
    /// Base canvas height for the rasteriser at zoom = 1. Scaled by
    /// the active zoom bucket when re-rasterising so the source has
    /// enough pixel detail to ROI-crop sharply at the current zoom.
    base_target_height_px: u32,
    /// Embedded face count from the TTC header (1 for plain TTF/OTF).
    face_count: u32,
    /// Currently rendered face. `current < face_count`.
    current: u32,
    /// Rasterised sample for the active face. Re-rasterised at higher
    /// resolution as zoom grows so [`ImageView`]'s ROI crop has enough
    /// source detail. Invalidates [`cache`] whenever it changes.
    decoded: DynamicImage,
    /// Zoom bucket the active `decoded` was rasterised for. Buckets
    /// step in integer zoom levels (1, 2, 3, …) so a 1.25× → 1.56×
    /// step doesn't trigger an expensive fontdue re-pass. Zoom out
    /// keeps the higher-resolution decoded — over-detail is fine; the
    /// ROI crop just resamples a sharper source.
    decoded_zoom_bucket: u32,
    view: ImageView,
    cache: Option<CachedFrame>,
}

impl SpecimenMode {
    pub(crate) fn new(
        bytes: Bytes,
        face_count: u32,
        decoded: DynamicImage,
        target_height_px: u32,
        config: ImageConfig,
    ) -> Self {
        Self {
            bytes,
            base_target_height_px: target_height_px,
            face_count: face_count.max(1),
            current: 0,
            decoded,
            decoded_zoom_bucket: 1,
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

    /// Re-rasterise the active face at higher resolution when the live
    /// zoom outgrows the current source detail. Down-zooming keeps the
    /// existing high-res decoded — the ROI crop just resamples a
    /// sharper source, no quality loss.
    fn ensure_decoded_for_zoom(&mut self, zoom: f32) {
        let bucket = integer_bucket(zoom);
        if bucket <= self.decoded_zoom_bucket {
            return;
        }
        let target = self.base_target_height_px.saturating_mul(bucket);
        if let Ok(new_image) = specimen::rasterise(&self.bytes, self.current, target) {
            self.decoded = new_image;
            self.decoded_zoom_bucket = bucket;
            self.cache = None;
        }
    }

    /// Step the active face by `delta` (wraps at both ends) and
    /// re-rasterise at the current zoom bucket so a face cycle
    /// preserves the active zoom's sharpness. A face that fontdue
    /// can't parse leaves the previous specimen in place rather than
    /// silently going blank.
    fn step_face(&mut self, delta: i32) -> bool {
        if self.face_count < 2 {
            return false;
        }
        let n = self.face_count as i32;
        let next = ((self.current as i32 + delta).rem_euclid(n)) as u32;
        if next == self.current {
            return false;
        }
        let target = self
            .base_target_height_px
            .saturating_mul(self.decoded_zoom_bucket);
        let Ok(new_image) = specimen::rasterise(&self.bytes, next, target) else {
            return false;
        };
        self.current = next;
        self.decoded = new_image;
        self.cache = None;
        true
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
        self.ensure_decoded_for_zoom(self.view.zoom().factor());
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
        let term = cell_size::term_size(cache.key.term_cols, cache.key.term_rows);
        let bounds = self.view.view_bounds(&cache.prep, term);
        let page_y = bounds.viewport_rows.saturating_sub(1);
        self.view.scroll(
            action,
            ScrollBounds::clamped(bounds.max_x, bounds.max_y, page_y),
        )
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        if self.face_count > 1 {
            SPECIMEN_ACTIONS_WITH_FACE_CYCLE
        } else {
            SPECIMEN_ACTIONS
        }
    }

    fn handle(&mut self, action: Action) -> Handled {
        if self.face_count > 1 {
            match action {
                Action::NextFace => {
                    return if self.step_face(1) {
                        Handled::Yes
                    } else {
                        Handled::No
                    };
                }
                Action::PrevFace => {
                    return if self.step_face(-1) {
                        Handled::Yes
                    } else {
                        Handled::No
                    };
                }
                _ => {}
            }
        }
        if let Some(h) = self.view.handle_config_cycle(action) {
            return h;
        }
        if let Some(cache) = &self.cache {
            let term = cell_size::term_size(cache.key.term_cols, cache.key.term_rows);
            let bounds = self.view.view_bounds(&cache.prep, term);
            if let Some(h) = self.view.handle_zoom(action, bounds) {
                return h;
            }
        }
        Handled::No
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let mut segs = self.view.status_segments(theme);
        if self.face_count > 1 {
            segs.insert(
                0,
                (
                    format!("Face {}/{}", self.current + 1, self.face_count),
                    theme.muted,
                ),
            );
        }
        segs
    }
}
