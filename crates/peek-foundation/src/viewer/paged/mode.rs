//! [`PagedImageMode<R>`] — the shared `Mode` impl for paged-image
//! documents (PDF pages, CBZ pages, EPS previews), generic over a
//! [`PageRenderer`]. Navigation, zoom, pan, config cycling, and the
//! pipe walk live here; the renderer owns page rasterisation and its
//! own caching. The building-block primitives (cache key, step logic,
//! config cycle, help consts) live in the parent module — EPUB's read
//! mode reuses those without this Mode impl.

use anyhow::Result;
use syntect::highlighting::Color;

use super::{
    CYCLE_BACKGROUND_HELP, CYCLE_FIT_HELP, CYCLE_IMAGE_MODE_HELP, PageRenderer, RenderArgs,
    cycle_image_config, pipe_walk_pages, step_paged, term_size_for,
};
use crate::output::PrintOutput;
use crate::theme::PeekTheme;
use crate::viewer::image_render::{ImageConfig, ScrollBounds, ViewBounds, ZoomPanState};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::viewer::ui::{Action, HelpEntry};

/// Mode-local help entries for [`PagedImageMode`]: page navigation plus
/// the shared image-config block plus zoom.
///
/// `ScrollLeft` / `ScrollRight` are listed explicitly even though the
/// scroll machinery handles them — dispatch goes through this slice to
/// find a key binding, and the global slice only carries vertical
/// scroll. Without this entry Left/Right never reach the mode.
const EXTRA_ACTIONS: &[HelpEntry] = &[
    (&[Action::Next, Action::Prev], "Next / previous page"),
    CYCLE_BACKGROUND_HELP,
    CYCLE_IMAGE_MODE_HELP,
    CYCLE_FIT_HELP,
    (
        &[Action::ToggleTextOverlay],
        "Toggle reconstructed-text overlay",
    ),
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Pan left / right (when zoomed or fit=FitHeight)",
    ),
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

/// Paged-image read mode generic over its [`PageRenderer`].
///
/// Shows one page at a time through the image pipeline; `n` / `p` step
/// pages, `b` / `m` / `f` cycle image config, `+` / `-` / `0` / `1`..`9`
/// zoom. The renderer owns whatever per-page caching makes sense for
/// its source (decoded source bitmap for CBZ, rasterized effective
/// grid for PDF) and returns the visible viewport's ASCII for the
/// current zoom + pan state — this mode no longer holds a rendered-
/// output cache of its own, so memory stays bounded by viewport
/// rather than effective grid (`viewport × zoom²`).
pub struct PagedImageMode<R: PageRenderer> {
    renderer: R,
    image_config: ImageConfig,
    /// Tab label. Defaults to "Read" for single-view paged documents
    /// (PDF / CBZ); EPS overrides it to distinguish its "Preview" and
    /// "Render" image tabs.
    label: &'static str,
    current: usize,
    warnings: Vec<String>,
    pan: ZoomPanState,
    /// Reconstructed-text overlay state (`o`). Only flippable when the
    /// renderer reports `supports_text_overlay`; stays false otherwise.
    text_overlay: bool,
    /// Last viewport rendered into, captured at the end of
    /// `render_window`. Read by `handle` / `scroll` to compute scroll
    /// bounds and zoom anchoring without re-running the renderer.
    last_viewport_cols: u32,
    last_viewport_rows: u32,
    /// Last effective-grid dims from the renderer. Used by `scroll`
    /// to clamp pan bounds without invoking the renderer.
    last_effective_cols: u32,
    last_effective_rows: u32,
}

impl<R: PageRenderer> PagedImageMode<R> {
    pub fn new(renderer: R, image_config: ImageConfig) -> Self {
        Self::with_label(renderer, image_config, "Read")
    }

    /// Like [`Self::new`] but with a caller-supplied tab label.
    pub fn with_label(renderer: R, image_config: ImageConfig, label: &'static str) -> Self {
        Self {
            renderer,
            image_config,
            label,
            current: 0,
            warnings: Vec::new(),
            pan: ZoomPanState::new(),
            text_overlay: false,
            last_viewport_cols: 0,
            last_viewport_rows: 0,
            last_effective_cols: 0,
            last_effective_rows: 0,
        }
    }
}

impl<R: PageRenderer> Mode for PagedImageMode<R> {
    fn id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn label(&self) -> &str {
        self.label
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, _rows: usize) -> Result<Window> {
        if self.renderer.page_count() == 0 {
            self.last_viewport_cols = 0;
            self.last_viewport_rows = 0;
            self.last_effective_cols = 0;
            self.last_effective_rows = 0;
            return Ok(Window {
                lines: Vec::new(),
                total: 0,
            });
        }
        let args = RenderArgs {
            term: term_size_for(ctx.term_cols, ctx.term_rows),
            zoom: self.pan.zoom,
            scroll_x: self.pan.scroll_x,
            scroll_y: self.pan.scroll_y,
            style_mode: ctx.peek_theme.style_mode,
            text_overlay: self.text_overlay,
        };
        let render =
            self.renderer
                .render_page(self.current, self.image_config, args, &mut self.warnings)?;
        self.last_viewport_cols = render.viewport_cols;
        self.last_viewport_rows = render.viewport_rows;
        self.last_effective_cols = render.effective_cols;
        self.last_effective_rows = render.effective_rows;
        // Clamp scroll against the freshly observed effective grid so
        // future redraws / scroll calls start from a valid origin.
        let max_x = render.effective_cols.saturating_sub(render.viewport_cols);
        let max_y = render.effective_rows.saturating_sub(render.viewport_rows);
        self.pan.scroll_x = self.pan.scroll_x.min(max_x);
        self.pan.scroll_y = self.pan.scroll_y.min(max_y);
        Ok(Window {
            lines: render.lines,
            total: render.effective_rows as usize,
        })
    }

    fn total_lines(&self) -> Option<usize> {
        if self.last_effective_rows == 0 {
            None
        } else {
            Some(self.last_effective_rows as usize)
        }
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        // Bail optimistically before the first render — without it we
        // have no effective-grid or viewport dims to clamp against.
        if self.last_viewport_cols == 0 || self.last_viewport_rows == 0 {
            return false;
        }
        let max_x = self
            .last_effective_cols
            .saturating_sub(self.last_viewport_cols);
        let max_y = self
            .last_effective_rows
            .saturating_sub(self.last_viewport_rows);
        let page_y = self.last_viewport_rows.saturating_sub(1);
        self.pan
            .scroll(action, ScrollBounds::clamped(max_x, max_y, page_y))
    }

    /// Print mode walks every page in order, separated by a blank line.
    /// Honors the cache so already-rendered pages reuse their output;
    /// the interactive view stays single-page. Forces zoom = 1× and
    /// pan = origin for the duration — pipe output is non-interactive,
    /// so the user's live zoom can't help and would only widen lines
    /// past the terminal.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let total = self.renderer.page_count();
        let saved_current = self.current;
        let saved_pan = std::mem::replace(&mut self.pan, ZoomPanState::new());
        let args = RenderArgs {
            term: term_size_for(ctx.term_cols, ctx.term_rows),
            zoom: self.pan.zoom,
            scroll_x: 0,
            scroll_y: 0,
            style_mode: ctx.peek_theme.style_mode,
            text_overlay: self.text_overlay,
        };
        let renderer = &self.renderer;
        let config = self.image_config;
        let warnings = &mut self.warnings;
        let res = pipe_walk_pages(out, total, |i, out| {
            let render = renderer.render_page(i, config, args, warnings)?;
            for line in &render.lines {
                out.write_line(line)?;
            }
            Ok(())
        });
        self.current = saved_current;
        self.pan = saved_pan;
        res
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn help_entries(&self) -> Vec<HelpEntry> {
        // Drop the overlay row when the source has no text layer —
        // the key is inert there (CBZ, image-only PDFs).
        EXTRA_ACTIONS
            .iter()
            .filter(|(keys, _)| {
                self.renderer.supports_text_overlay() || !keys.contains(&Action::ToggleTextOverlay)
            })
            .copied()
            .collect()
    }

    fn handle(&mut self, action: Action) -> Handled {
        if action == Action::ToggleTextOverlay {
            if !self.renderer.supports_text_overlay() {
                return Handled::No;
            }
            self.text_overlay = !self.text_overlay;
            return Handled::Yes;
        }
        if let Some(h) = cycle_image_config(action, &mut self.image_config) {
            // Fit change invalidates the rendered grid; reset pan.
            if matches!(action, Action::CycleFitMode) {
                self.pan.reset_pan();
            }
            return h;
        }
        let zoom_bounds = ViewBounds {
            max_x: self
                .last_effective_cols
                .saturating_sub(self.last_viewport_cols),
            max_y: self
                .last_effective_rows
                .saturating_sub(self.last_viewport_rows),
            viewport_cols: self.last_viewport_cols,
            viewport_rows: self.last_viewport_rows,
        };
        if let Some(h) = self.pan.handle_zoom(action, zoom_bounds) {
            return h;
        }
        let count = self.renderer.page_count();
        match action {
            Action::Next => {
                let h = step_paged(&mut self.current, count, 1);
                if matches!(h, Handled::YesResetScroll) {
                    self.pan.reset_pan();
                }
                h
            }
            Action::Prev => {
                let h = step_paged(&mut self.current, count, -1);
                if matches!(h, Handled::YesResetScroll) {
                    self.pan.reset_pan();
                }
                h
            }
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let count = self.renderer.page_count();
        if count == 0 {
            return Vec::new();
        }
        let mut out = vec![(format!("page {}/{}", self.current + 1, count), theme.muted)];
        if !self.pan.zoom.is_one() {
            out.push((self.pan.zoom.label(), theme.label));
        }
        if self.text_overlay {
            out.push(("text".to_string(), theme.label));
        }
        out
    }

    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> {
        if self.renderer.page_count() <= 1 {
            return Vec::new();
        }
        vec!["n/p:page"]
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::super::PagedRender;
    use super::*;
    use crate::theme::StyleMode;
    use crate::viewer::image_render::{Background, FitMode, ImageMode};

    /// Regression: PagedImageMode must advertise `ScrollLeft` /
    /// `ScrollRight` in its `EXTRA_ACTIONS` slice so the global key
    /// dispatcher can match Left/Right and route them to the mode.
    /// `GLOBAL_ACTIONS` only carries vertical scroll; horizontal pan
    /// is opt-in per mode (same pattern as ImageRenderMode +
    /// AnimationMode + SvgAnimationMode + SpecimenMode).
    #[test]
    fn paged_extra_actions_carries_horizontal_scroll() {
        let actions: Vec<Action> = EXTRA_ACTIONS
            .iter()
            .flat_map(|(keys, _)| keys.iter().copied())
            .collect();
        assert!(
            actions.contains(&Action::ScrollLeft),
            "EXTRA_ACTIONS must include ScrollLeft so Left routes through dispatch"
        );
        assert!(
            actions.contains(&Action::ScrollRight),
            "EXTRA_ACTIONS must include ScrollRight so Right routes through dispatch"
        );
    }

    /// Regression: horizontal scroll under zoom on PagedImageMode. With
    /// a wide effective grid (e.g. zoomed CBZ page), pressing `Right`
    /// must advance scroll_x and the renderer must see the new offset
    /// on the next call.
    #[test]
    fn paged_mode_horizontal_scroll_under_zoom() {
        use crate::info::{FileInfo, NoExtras, RenderOptions};
        use crate::theme::{PeekTheme, PeekThemeName, load_embedded_theme};
        use std::cell::Cell;

        struct WideRenderer {
            last_scroll_x: Cell<u32>,
        }
        impl PageRenderer for WideRenderer {
            fn page_count(&self) -> usize {
                1
            }
            fn render_page(
                &self,
                _idx: usize,
                _config: ImageConfig,
                args: RenderArgs,
                _warnings: &mut Vec<String>,
            ) -> Result<PagedRender> {
                self.last_scroll_x.set(args.scroll_x);
                // Effective grid 160×40, viewport (clamped to 80-col
                // terminal) = 80×40. Renderer fills the viewport with
                // 'X' so the test can spot-check shape.
                Ok(PagedRender {
                    lines: (0..40).map(|_| "X".repeat(80)).collect(),
                    effective_cols: 160,
                    effective_rows: 40,
                    viewport_cols: 80,
                    viewport_rows: 40,
                })
            }
        }

        let cfg = ImageConfig {
            mode: ImageMode::from_str("block"),
            width: 0,
            background: Background::from_str("auto"),
            margin: 0,
            style_mode: StyleMode::Plain,
            edge_density: 0.1,
            fit: FitMode::Contain,
        };
        let renderer = WideRenderer {
            last_scroll_x: Cell::new(0),
        };
        let mut mode = PagedImageMode::new(renderer, cfg);

        let syntect = load_embedded_theme(PeekThemeName::default().tmtheme_source());
        let peek_theme = PeekTheme::from_syntect(&syntect);
        let file_info = FileInfo {
            file_name: String::new(),
            path: String::new(),
            size_bytes: 0,
            mimes: Vec::new(),
            warnings: Vec::new(),
            modified: None,
            created: None,
            permissions: None,
            compression: None,
            extras: Box::new(NoExtras),
        };
        let ctx = RenderCtx {
            file_info: &file_info,
            theme_name: PeekThemeName::default(),
            peek_theme: &peek_theme,
            render_opts: RenderOptions::default(),
            term_cols: 80,
            term_rows: 40,
        };

        // Initial render populates the effective-grid + viewport bounds
        // on the mode so scroll has something to clamp against.
        let win = mode.render_window(&ctx, 0, 40).expect("render");
        assert_eq!(win.lines.len(), 40);
        assert_eq!(mode.last_viewport_cols, 80);
        assert_eq!(mode.last_effective_cols, 160);
        assert_eq!(mode.pan.scroll_x, 0);
        assert_eq!(mode.renderer.last_scroll_x.get(), 0);

        // Right arrow: scroll_x advances by HSTEP (= 4 cells).
        assert!(Mode::scroll(&mut mode, Action::ScrollRight));
        assert_eq!(mode.pan.scroll_x, 4);

        // Next render hands the updated scroll_x to the renderer so the
        // ROI path picks up the pan.
        let _ = mode.render_window(&ctx, 0, 40).expect("render");
        assert_eq!(mode.renderer.last_scroll_x.get(), 4);
    }
}
