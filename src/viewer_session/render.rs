//! Render-cache layer of the interactive viewer: [`RenderedView`] (one
//! mode's cached windowed render), the render / cache-fill path with its
//! two failure-recovery steps (magic-byte re-detection, degrade to Hex),
//! caller-side scroll math for modes that don't own scroll, resize
//! invalidation, and the final `draw` to the terminal.

use std::io;

use anyhow::Result;

use peek_foundation::viewer::modes::{ModeId, RenderCtx};
use peek_foundation::viewer::ui::{content_rows, terminal_cols};

use super::state::ViewerState;

/// One mode's most recent windowed render. The `lines` field is the
/// exact slice that should be drawn at the top of the viewport; the
/// `scroll_at` and `rows_at` fields are the inputs the mode was given,
/// used as the cache key. `total` is the full-source line count so
/// scroll math (max_scroll, Bottom jump) doesn't need to re-render.
pub(crate) struct RenderedView {
    pub(super) lines: Vec<String>,
    pub(super) scroll_at: usize,
    pub(super) rows_at: usize,
    pub(super) total: usize,
}

/// Concise one-line summary of a render failure for the status flash /
/// warning row: the deepest cause in the error chain (e.g. the decoder's
/// "CRC error: …"), which is more actionable than the outer
/// "failed to render" wrapper.
fn render_failure_cause(err: &anyhow::Error) -> String {
    err.chain()
        .last()
        .map_or_else(|| err.to_string(), |c| c.to_string())
}

impl ViewerState {
    // ---------------------------------------------------------------------
    // Rendering
    // ---------------------------------------------------------------------

    pub(crate) fn ensure_active_rendered(&mut self) -> Result<()> {
        let (active, scroll) = {
            let f = self.frame();
            (f.active, f.scroll[f.active])
        };
        let rows = content_rows();
        let cache_hit = self.frame().views[active]
            .as_ref()
            .is_some_and(|v| v.scroll_at == scroll && v.rows_at == rows);
        if !cache_hit {
            match self.render_active() {
                Ok(view) => {
                    self.frame_mut().views[active] = Some(view);
                }
                Err(e) => {
                    // Frame may have been built from a name-biased detect
                    // (file extension lied about the content). Try
                    // magic-byte-only re-detection once; if it yields a
                    // different file type, rebuild the frame and retry
                    // the render. Applies uniformly to root and nested
                    // descended frames.
                    let render_err =
                        if !self.frame().retry_attempted && self.retry_frame_detection()? {
                            let active = self.frame().active;
                            match self.render_active() {
                                Ok(view) => {
                                    self.frame_mut().views[active] = Some(view);
                                    return Ok(());
                                }
                                // Re-detected type also fails to render;
                                // fall through to the Hex degrade below
                                // with the new error.
                                Err(e2) => e2,
                            }
                        } else {
                            e
                        };
                    if let Some(view) = self.degrade_active_to_hex(&render_err)? {
                        // Re-detection didn't help (or already ran): the
                        // active mode genuinely can't render this input
                        // (corrupt image, malformed payload, …). Rather
                        // than abort the whole viewer, drop to the
                        // universal Hex view and surface the error as a
                        // warning. `degrade_active_to_hex` repointed
                        // `active` at Hex before rendering.
                        let active = self.frame().active;
                        self.frame_mut().views[active] = Some(view);
                    } else {
                        return Err(render_err);
                    }
                }
            }
        }
        Ok(())
    }

    /// Re-detect the active frame's source without using its path /
    /// entry name, rebuild modes + file_info if the classification
    /// changed, and reset cached views. Sets `retry_attempted` whether
    /// or not the classification changed so the caller doesn't loop.
    /// Returns `Ok(true)` when the frame was rebuilt and is worth
    /// re-rendering, `Ok(false)` when re-detection didn't change the
    /// type.
    fn retry_frame_detection(&mut self) -> Result<bool> {
        let retried = {
            let frame = self.frame();
            match peek_detect::detect_ignore_name(&frame.source) {
                Ok(d) if d.file_type != frame.detected.file_type => d,
                _ => {
                    self.frame_mut().retry_attempted = true;
                    return Ok(false);
                }
            }
        };
        // Re-detect-on-magic may surface a bare codec the name hid —
        // resolve transparently so the rebuilt frame renders the
        // decompressed inner content.
        let (source_clone, retried) =
            peek_detect::resolve_transparent(self.frame().source.clone(), retried);
        let modes = (self.mode_builder)(&source_clone, &retried)?;
        let file_info = crate::gather::gather(&source_clone, &retried)?;
        let frame = self.frame_mut();
        frame.source = source_clone;
        frame.detected = retried;
        frame.file_info = file_info;
        frame.reseed_from_modes(modes);
        frame.retry_attempted = true;
        // Drop the ScreenBuffer's row-diff cache so the next draw
        // repaints every row — the rebuilt frame's mode set, status
        // line, and content can differ from whatever the parent frame
        // (or earlier render attempt) left on screen.
        self.screen.invalidate();
        Ok(true)
    }

    /// Last-resort fallback when the active mode cannot render the input
    /// (e.g. a corrupt image, a malformed structured payload). Repoints
    /// `active` at the always-present Hex view, records `err` as a frame
    /// warning (so the Info view and the breadcrumb `!` mark surface it),
    /// flashes a one-line notice, and returns the Hex render so the caller
    /// can cache it.
    ///
    /// Returns `Ok(None)` when there's nothing safer to fall back to —
    /// the failed mode *is* Hex, or no Hex view exists (directories) — so
    /// the caller propagates the original error instead of looping.
    fn degrade_active_to_hex(&mut self, err: &anyhow::Error) -> Result<Option<RenderedView>> {
        let f = self.frame();
        let failed = f.active;
        let Some(hex_idx) = f.mode_index(ModeId::Hex) else {
            return Ok(None);
        };
        if failed == hex_idx {
            return Ok(None);
        }
        let warning = format!("{}: {}", f.modes[failed].label(), render_failure_cause(err));
        let f = self.frame_mut();
        if !f.file_info.warnings.contains(&warning) {
            f.file_info.warnings.push(warning);
            if let Some(idx) = f.mode_index(ModeId::Info) {
                f.views[idx] = None;
            }
        }
        // The broken mode is no longer the home view: aux toggles must not
        // bounce back into it.
        if f.last_primary == Some(failed) {
            f.last_primary = None;
        }
        f.active = hex_idx;
        self.flash = Some(format!("cannot display — {}", render_failure_cause(err)));
        let view = self.render_active()?;
        Ok(Some(view))
    }

    pub(crate) fn invalidate_active(&mut self) {
        let f = self.frame_mut();
        let active = f.active;
        f.views[active] = None;
    }

    /// Themes / color modes are global; staling every frame's view
    /// cache stops a pop-into-old-frame from showing stale colours.
    pub(super) fn invalidate_all_views(&mut self) {
        for frame in &mut self.frames {
            for slot in &mut frame.views {
                *slot = None;
            }
        }
    }

    fn render_active(&mut self) -> Result<RenderedView> {
        let theme_name = self.current_theme;
        let render_opts = self.render_opts;
        let term_cols_v = terminal_cols();
        let rows = content_rows();
        // Borrow theme separately from the frame's mutable borrow —
        // `peek_theme` lives on `self`, not on the frame, so the two
        // disjoint accesses don't alias.
        let peek_theme = self.peek_theme.clone();
        let f = self.frame_mut();
        let active = f.active;
        let scroll = f.scroll[active];
        let window = {
            let ctx = RenderCtx {
                file_info: &f.file_info,
                theme_name,
                peek_theme: &peek_theme,
                render_opts,
                term_cols: term_cols_v,
                term_rows: rows,
            };
            f.modes[active].render_window(&ctx, scroll, rows)?
        };
        // Append only warnings not already recorded. Paged renderers
        // re-emit the same per-frame warning on every redraw when a page
        // can't be rendered (e.g. a failed Ghostscript / image decode);
        // deduping keeps `file_info.warnings` from growing without bound
        // and avoids needless Info-view cache invalidation each frame.
        let new_warnings = f.modes[active].take_warnings();
        let mut added_any = false;
        for w in new_warnings {
            if !f.file_info.warnings.contains(&w) {
                f.file_info.warnings.push(w);
                added_any = true;
            }
        }
        if added_any && let Some(idx) = f.mode_index(ModeId::Info) {
            f.views[idx] = None;
        }
        Ok(RenderedView {
            lines: window.lines,
            scroll_at: scroll,
            rows_at: rows,
            total: window.total,
        })
    }

    // ---------------------------------------------------------------------
    // Resize
    // ---------------------------------------------------------------------

    pub(crate) fn handle_resize(&mut self) {
        let cols = terminal_cols();
        let rows = content_rows();
        for frame in &mut self.frames {
            for (i, m) in frame.modes.iter_mut().enumerate() {
                m.on_resize(cols, rows);
                if m.rerender_on_resize() {
                    frame.views[i] = None;
                }
            }
        }
        self.screen.invalidate();
    }

    // ---------------------------------------------------------------------
    // Line scrolling (used when active mode does NOT own scroll)
    // ---------------------------------------------------------------------

    pub(super) fn max_scroll(&self) -> usize {
        let f = self.frame();
        let total = f.views[f.active].as_ref().map_or(0, |v| v.total);
        total.saturating_sub(content_rows())
    }

    pub(super) fn prepare_total(&mut self) -> Result<()> {
        let (active, total_lines, has_view) = {
            let f = self.frame();
            (
                f.active,
                f.modes[f.active].total_lines(),
                f.views[f.active].is_some(),
            )
        };
        if let Some(n) = total_lines {
            let needs_seed = self.frame().views[active]
                .as_ref()
                .is_none_or(|v| v.total != n);
            if needs_seed {
                self.frame_mut().views[active] = Some(RenderedView {
                    lines: Vec::new(),
                    scroll_at: usize::MAX,
                    rows_at: content_rows(),
                    total: n,
                });
            }
            return Ok(());
        }
        if !has_view {
            self.ensure_active_rendered()?;
        }
        Ok(())
    }

    pub(super) fn scroll_by(&mut self, delta: isize) -> Result<()> {
        if self.frame().modes[self.frame().active].owns_scroll() {
            return Ok(());
        }
        self.prepare_total()?;
        let max = self.max_scroll();
        let f = self.frame_mut();
        let active = f.active;
        let s = &mut f.scroll[active];
        *s = if delta < 0 {
            s.saturating_sub((-delta) as usize)
        } else {
            (*s + delta as usize).min(max)
        };
        Ok(())
    }

    pub(super) fn page(&mut self, direction: isize) -> Result<()> {
        if self.frame().modes[self.frame().active].owns_scroll() {
            return Ok(());
        }
        self.prepare_total()?;
        let step = content_rows().saturating_sub(1);
        let max = self.max_scroll();
        let f = self.frame_mut();
        let active = f.active;
        let s = &mut f.scroll[active];
        *s = if direction < 0 {
            s.saturating_sub(step)
        } else {
            (*s + step).min(max)
        };
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Drawing
    // ---------------------------------------------------------------------

    pub(crate) fn draw(&mut self, stdout: &mut io::Stdout, status: &str) -> Result<()> {
        let reset_bytes = self.peek_theme.style_mode.reset_bytes();
        // Borrow `frames` by field (not via `self.frame()`) so the
        // borrow stays disjoint from `&mut self.screen` below.
        let f = self.frames.last().expect("non-empty stack");
        let lines: &[String] = f.views[f.active].as_ref().map_or(&[], |v| &v.lines);
        self.screen.draw(stdout, lines, status, reset_bytes)
    }
}
