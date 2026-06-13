//! Interactive window rendering for [`ContentMode`](super::content::ContentMode).
//!
//! [`WindowRenderer`] is a per-render bundle of `ContentMode` field
//! borrows — built at the top of `render_window`, dropped before the
//! final re-clamp. Two steps: [`prepare`](WindowRenderer::prepare)
//! materialises the styled logical lines covering the visible window
//! (branch-specific sequencing — highlighter catch-up for raw, cache
//! refresh for pretty), then [`emit`](WindowRenderer::emit) walks them
//! through the shared geometry (search overlay, soft-wrap / h-slice,
//! gutter prefix) into visual rows.
//!
//! The pipe-mode sibling is [`content_pipe`](super::content_pipe);
//! keystroke handling, search wiring, and the `Mode` impl stay in
//! `content.rs`.

use std::rc::Rc;

use anyhow::Result;

use super::RenderCtx;
use super::content_rendering::RenderingMode;
use super::gutter::Gutter;
use super::pretty_view::{PrettyView, SyntaxRef};
use crate::input::LineSource;
use crate::theme::{PeekTheme, ThemeManager};
use crate::viewer::LineStreamHighlighter;
use crate::viewer::search::{self, SearchState};
use crate::viewer::ui::{slice_styled_h, wrap_styled};
use crate::viewer::wrap_scroll::{PrettyLines, WrapScroll};

/// Visible columns left for content after the line-number gutter. The
/// wrap geometry and h-scroll slicing all work in this width. A free
/// function so `ContentMode`'s search wiring shares the same math.
pub(super) fn usable_width(cols: usize, gutter: &Gutter, total: usize) -> usize {
    cols.max(1)
        .saturating_sub(gutter.visible_width(total))
        .max(1)
}

/// The `ContentMode` field borrows the window-render path touches.
/// The fields are disjoint from what the post-render `clamp_top` needs
/// mutably (`wrap`), so the renderer is built, used, and dropped
/// within `render_window`.
pub(super) struct WindowRenderer<'a> {
    pub rendering: &'a mut RenderingMode,
    pub line_source: &'a LineSource,
    pub highlighter: &'a mut Option<LineStreamHighlighter>,
    pub syntax_token: Option<&'a str>,
    pub theme_manager: &'a Rc<ThemeManager>,
    pub wrap: &'a WrapScroll,
    pub gutter: &'a Gutter,
    pub search: Option<&'a SearchState>,
    pub cols: usize,
}

impl WindowRenderer<'_> {
    /// Materialise styled lines covering the visible window of the
    /// active branch — `(styled[top_logical..end], top_logical, total)`.
    /// Empty `styled` when the branch is empty or `rows == 0`; `total`
    /// still reports the active output's line count so the status line
    /// tracks document size. Branch-specific sequencing (highlighter
    /// catch-up for raw, cache refresh for pretty) lives here; the
    /// shared geometry walker [`emit`](Self::emit) consumes the result.
    pub(super) fn prepare(
        &mut self,
        ctx: &RenderCtx,
        rows: usize,
    ) -> Result<(Vec<String>, usize, usize)> {
        // Active pretty branch (Some only when Showing::Pretty + parsed):
        // refresh its rendered-line cache for the current theme, then
        // slice the window. The two-step ensure_rendered then
        // rendered_lines pattern stays — the borrow on `pretty_mut` for
        // ensure_rendered conflicts with the syntax_token / theme_manager
        // borrows alongside it, so we split the access.
        if self.rendering.is_pretty_ready() {
            let syntax = self.syntax_token.map(|token| SyntaxRef {
                token,
                theme_manager: self.theme_manager,
            });
            if let Some(pv) = self.rendering.active_pretty_mut() {
                pv.ensure_rendered(ctx.theme_name, ctx.peek_theme.style_mode, syntax)?;
            }
            let lines = self
                .rendering
                .active_pretty()
                .and_then(PrettyView::rendered_lines);
            let total = lines.as_ref().map_or(0, PrettyLines::len);
            if total == 0 || rows == 0 {
                return Ok((Vec::new(), 0, total));
            }
            let lines = lines.expect("non-zero total implies Some");
            let top_logical = self.wrap.top_logical().min(total - 1);
            let lookahead = if self.wrap.soft_wrap() {
                rows.saturating_add(8)
            } else {
                rows
            };
            let end = top_logical.saturating_add(lookahead).min(total);
            let window: Vec<String> = (top_logical..end)
                .filter_map(|i| lines.get(i).map(str::to_owned))
                .collect();
            return Ok((window, top_logical, total));
        }

        let total = self.line_source.total_lines();
        if total == 0 || rows == 0 {
            return Ok((Vec::new(), 0, total));
        }
        let top_logical = self.wrap.top_logical().min(total - 1);
        // Lookahead buffer: each visible logical line yields ≥ 1 visual
        // row so `rows` lines is enough; the small margin absorbs cases
        // where `first_skip` swallows leading segments of the top line.
        let lookahead = if self.wrap.soft_wrap() {
            rows.saturating_add(8)
        } else {
            rows
        };
        let end = top_logical.saturating_add(lookahead).min(total);

        if let Some(hl) = self.highlighter.as_mut() {
            let theme_changed = hl.active_theme() != ctx.theme_name;
            if theme_changed || hl.at() > top_logical {
                hl.reset(ctx.theme_name);
            }
        }
        let start_at = self.highlighter.as_ref().map_or(top_logical, |h| h.at());
        if start_at >= end {
            return Ok((Vec::new(), top_logical, total));
        }
        let raw_lines = self.line_source.window(start_at..end)?;
        let style_mode = ctx.peek_theme.style_mode;
        let catchup = top_logical.saturating_sub(start_at);
        let mut styled: Vec<String> = Vec::with_capacity(raw_lines.len().saturating_sub(catchup));
        for (offset, raw) in raw_lines.iter().enumerate() {
            // Pre-`top_logical` lines feed the highlighter for state
            // continuity but their styled output is thrown away.
            let out = if let Some(hl) = self.highlighter.as_mut() {
                hl.feed(raw, style_mode)?
            } else {
                raw.clone()
            };
            if offset >= catchup {
                styled.push(out);
            }
        }
        Ok((styled, top_logical, total))
    }

    /// Walk pre-styled visible lines through
    /// [`emit_visual_rows`](Self::emit_visual_rows). The styled slice
    /// covers `[top_logical .. top_logical + styled.len())`.
    pub(super) fn emit(
        &self,
        ctx: &RenderCtx,
        rows: usize,
        styled: &[String],
        top_logical: usize,
        total: usize,
    ) -> Vec<String> {
        let usable = usable_width(self.cols, self.gutter, total);
        let mut first_skip = self.wrap.first_skip();
        let mut emitted: Vec<String> = Vec::with_capacity(rows);
        for (offset, line) in styled.iter().enumerate() {
            let line_idx = top_logical + offset;
            let stop = self.emit_visual_rows(
                &mut emitted,
                rows,
                line_idx,
                line,
                total,
                ctx.peek_theme,
                usable,
                first_skip,
            );
            first_skip = 0;
            if stop {
                break;
            }
        }
        emitted
    }

    /// Convert one styled logical line into one or more visual rows
    /// (each composed with gutter), respecting wrap / h-scroll mode.
    /// `first_skip` skips that many leading wrap segments — used for
    /// the very first emitted line so the viewport can start mid-wrap
    /// when the user has scrolled to a `top_sub_row > 0`. Returns
    /// `true` when `out.len() >= max_rows` after pushing.
    #[allow(clippy::too_many_arguments)]
    fn emit_visual_rows(
        &self,
        out: &mut Vec<String>,
        max_rows: usize,
        line_idx: usize,
        styled: &str,
        total: usize,
        peek_theme: &PeekTheme,
        usable_width: usize,
        first_skip: usize,
    ) -> bool {
        // Paint search-match backgrounds onto the logical line before it
        // is wrapped / h-sliced — wrap_styled and slice_styled_h carry
        // the background escapes across their cuts.
        let overlaid;
        let styled: &str = match self.search.and_then(|s| s.line_overlay(line_idx)) {
            Some((ranges, current)) => {
                overlaid = search::overlay_matches(styled, &ranges, current, peek_theme);
                &overlaid
            }
            None => styled,
        };
        let line_num = line_idx + 1;
        if !self.wrap.soft_wrap() {
            let body = slice_styled_h(styled, self.wrap.h_scroll(), usable_width);
            let prefix = self.gutter.prefix(Some(line_num), total, peek_theme);
            out.push(format!("{prefix}{body}"));
            return out.len() >= max_rows;
        }
        let segments = wrap_styled(styled, usable_width);
        for (seg_idx, seg) in segments.iter().enumerate() {
            if seg_idx < first_skip {
                continue;
            }
            let prefix = if seg_idx == 0 {
                self.gutter.prefix(Some(line_num), total, peek_theme)
            } else {
                self.gutter.prefix(None, total, peek_theme)
            };
            out.push(format!("{prefix}{seg}"));
            if out.len() >= max_rows {
                return true;
            }
        }
        false
    }
}
