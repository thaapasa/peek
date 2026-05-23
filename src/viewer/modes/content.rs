use std::rc::Rc;

use anyhow::Result;
use syntect::highlighting::Color;

use super::content_rendering::{RenderingMode, Showing};
use super::gutter::Gutter;
use super::pretty_view::{PrettyView, SyntaxRef};
use super::{Handled, Mode, ModeId, NEXT_PREV_MATCH_HELP, Position, RenderCtx, Window};
use crate::input::detect::StructuredFormat;
use crate::input::{InputSource, LineSource};
use crate::output::PrintOutput;
use crate::theme::{PeekTheme, PeekThemeName, ThemeManager};
use crate::viewer::LineStreamHighlighter;
use crate::viewer::search::{self, SearchState};
use crate::viewer::ui::{Action, HelpEntry, slice_styled_h, wrap_styled};
use crate::viewer::wrap_scroll::{LineView, WrapScroll};

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;

/// Content view: text, syntax-highlighted source, pretty-printed structured
/// data, or SVG XML source.
///
/// Raw mode streams from a `LineSource` (anchor-indexed line iterator
/// over the input). Each `render_window` fetches just the visible window
/// of lines; multi-GB files never load into memory. With a syntax token,
/// a forward-only `LineStreamHighlighter` is driven across the window;
/// backward scrolls past its current cursor reset and replay from line 0.
///
/// Pretty mode lives in [`PrettyView`] — a whole-document parse plus a
/// rendered-line cache, size-capped so a multi-GB JSON-shaped log can't
/// OOM — owned inside [`RenderingMode::Either`].
///
/// `r` flips the active output via [`RenderingMode::toggle`] when the
/// rendering is `Either` — structured files (JSON/YAML/TOML/XML) and SVG
/// XML, where raw vs pretty is a meaningful user choice. Source code /
/// plain text are `RawOnly`, so `r` is inert. The active sub-state
/// (Pretty / Raw) shows up as a status-line segment.
pub(crate) struct ContentMode {
    source: InputSource,
    line_source: LineSource,
    /// Forward-only syntect feeder for raw-mode highlighting. `None` when
    /// the view has no associated syntax (plain text, --plain mode).
    highlighter: Option<LineStreamHighlighter>,

    /// Which output is showing (Raw / Pretty) plus the lazy pretty-parse
    /// machinery when a pretty form exists. See [`RenderingMode`].
    rendering: RenderingMode,

    /// Warnings produced during render that haven't been collected by
    /// `ViewerState` yet — drained on every `take_warnings` call.
    pending_warnings: Vec<String>,
    syntax_token: Option<String>,
    theme_manager: Rc<ThemeManager>,
    /// Line-number gutter — its on/off state plus the painting logic.
    gutter: Gutter,
    label: &'static str,

    /// Wrap-aware scroll position — logical line, visual sub-row, and
    /// horizontal pan. Owned scroll state; replaces
    /// `ViewerState::scroll[active]` for ContentMode. Soft-wrap is on by
    /// default: vertical scroll then moves visual rows and the gutter
    /// blanks continuation rows; off, lines truncate and Left/Right pan.
    wrap: WrapScroll,
    /// Last terminal column count seen — set on every render and via
    /// `on_resize`. `scroll()` reads this rather than querying the
    /// terminal directly. (HexMode follows the same pattern.)
    cached_cols: usize,
    /// Last terminal row count seen (content area height).
    cached_rows: usize,

    /// Active text search, or `None`. Match positions are in the active
    /// output's line domain (raw or pretty); cleared when that domain
    /// changes (the raw/pretty toggle).
    search: Option<SearchState>,
}

/// Per-view configuration for a [`ContentMode`] — the knobs that vary
/// between a source-code view, a structured-data view, and a plain
/// paired-source view. Built in the compose path and consumed once by
/// [`ContentMode::new`]; it isn't stored. `Default` is the plain-text
/// shape, so a caller spells out only what differs from it.
pub(crate) struct ContentModeConfig {
    /// Status-line label.
    pub label: &'static str,
    /// syntect token for raw-mode highlighting. `None` → no highlighting.
    pub syntax_token: Option<String>,
    /// Structured format to pretty-print as. `None` → no pretty form,
    /// and `r` (raw/pretty toggle) is inert.
    pub pretty_target: Option<StructuredFormat>,
    /// Start in pretty view. Ignored when `pretty_target` is `None`.
    pub start_pretty: bool,
    /// Start with the line-number gutter visible.
    pub line_numbers: bool,
}

impl Default for ContentModeConfig {
    fn default() -> Self {
        Self {
            label: "Content",
            syntax_token: None,
            pretty_target: None,
            start_pretty: false,
            line_numbers: false,
        }
    }
}

const RAW_TOGGLE_ACTIONS: &[HelpEntry] = &[
    (&[Action::ToggleRawSource], "Toggle raw / pretty"),
    (&[Action::ToggleLineNumbers], "Toggle line numbers"),
    (&[Action::ToggleSoftWrap], "Toggle soft wrap"),
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Pan left / right (wrap off)",
    ),
    (&[Action::OpenSearch], "Search"),
    NEXT_PREV_MATCH_HELP,
];

const LINE_NUMBER_ACTIONS: &[HelpEntry] = &[
    (&[Action::ToggleLineNumbers], "Toggle line numbers"),
    (&[Action::ToggleSoftWrap], "Toggle soft wrap"),
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Pan left / right (wrap off)",
    ),
    (&[Action::OpenSearch], "Search"),
    NEXT_PREV_MATCH_HELP,
];

/// Pick the active output into a [`LineView`] borrow. Pretty only when
/// the rendering is `Either { showing: Pretty }` *and* `PrettyView`'s
/// rendered cache is built — `Pretty(&[])` before the first pretty
/// render keeps the geometry seeing an empty view.
///
/// A free function, not a `&self` method on `ContentMode`: it borrows
/// only the line-data fields, leaving `self.wrap` free for the `&mut`
/// borrow the geometry methods take alongside.
fn active_view<'a>(rendering: &'a RenderingMode, line_source: &'a LineSource) -> LineView<'a> {
    match rendering {
        RenderingMode::Either {
            showing: Showing::Pretty,
            pretty,
        } => LineView::Pretty(pretty.rendered_lines().unwrap_or(&[])),
        _ => LineView::Raw(line_source),
    }
}

impl ContentMode {
    pub(crate) fn new(
        source: InputSource,
        line_source: LineSource,
        theme_manager: Rc<ThemeManager>,
        initial_theme: PeekThemeName,
        cfg: ContentModeConfig,
    ) -> Self {
        let highlighter = cfg.syntax_token.as_ref().map(|t| {
            LineStreamHighlighter::new(t.clone(), Rc::clone(&theme_manager), initial_theme)
        });
        let rendering = match cfg.pretty_target {
            Some(target) => RenderingMode::Either {
                showing: if cfg.start_pretty {
                    Showing::Pretty
                } else {
                    Showing::Raw
                },
                pretty: PrettyView::new(target),
            },
            None => RenderingMode::RawOnly,
        };
        Self {
            source,
            line_source,
            highlighter,
            rendering,
            pending_warnings: Vec::new(),
            syntax_token: cfg.syntax_token,
            theme_manager,
            gutter: Gutter::new(cfg.line_numbers),
            label: cfg.label,
            wrap: WrapScroll::new(true),
            cached_cols: 0,
            cached_rows: 0,
            search: None,
        }
    }

    /// Visible columns left for content after the line-number gutter.
    /// The wrap geometry and h-scroll slicing all work in this width.
    fn usable_width(&self, total: usize) -> usize {
        self.cached_cols
            .max(1)
            .saturating_sub(self.gutter.visible_width(total))
            .max(1)
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
        let styled: &str = match self.search.as_ref().and_then(|s| s.line_overlay(line_idx)) {
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

    /// Total logical line count of the currently-active output.
    fn current_total(&self) -> usize {
        active_view(&self.rendering, &self.line_source).total()
    }

    /// Re-clamp the wrap position against the active output so it never
    /// sits past the effective bottom. Called after every scroll
    /// mutation and at the end of each render — a resize or theme cycle
    /// can change wrap segment counts and strand the viewport.
    fn clamp_top(&mut self) {
        let cl = active_view(&self.rendering, &self.line_source);
        let usable = self.usable_width(cl.total());
        let rows = self.cached_rows.max(1);
        self.wrap.clamp(&cl, usable, rows);
    }

    /// Parse the pretty branch if pretty mode is active and untried. On
    /// a size-cap refusal, force the rendering back to raw so position
    /// tracking and the status line reflect the now-permanent fallback.
    fn ensure_pretty_parsed(&mut self) {
        if !self.rendering.showing_pretty() {
            return;
        }
        let total_bytes = self.line_source.total_bytes();
        let Some(pv) = self.rendering.pretty_mut() else {
            return;
        };
        pv.ensure_parsed(&self.source, total_bytes, &mut self.pending_warnings);
        if pv.cap_exceeded() {
            self.rendering.force_raw();
        }
    }

    /// Materialise styled lines covering the visible window of the
    /// active branch — `(styled[top_logical..end], top_logical, total)`.
    /// `None` when the branch is empty or `rows == 0`. Branch-specific
    /// sequencing (highlighter catch-up for raw, cache refresh for
    /// pretty) lives here; the shared geometry walker
    /// [`emit_window`](Self::emit_window) consumes the result.
    fn prepare_window(
        &mut self,
        ctx: &RenderCtx,
        rows: usize,
        pretty_ready: bool,
    ) -> Result<Option<(Vec<String>, usize, usize)>> {
        if pretty_ready {
            // Refresh the rendered-line cache for the active theme.
            {
                let syntax = self.syntax_token.as_deref().map(|token| SyntaxRef {
                    token,
                    theme_manager: &self.theme_manager,
                });
                let pv = self.rendering.pretty_mut().expect("pretty branch present");
                pv.ensure_rendered(ctx.theme_name, ctx.peek_theme.style_mode, syntax)?;
            }
            let lines: &[String] = self
                .rendering
                .pretty()
                .and_then(PrettyView::rendered_lines)
                .expect("pretty cache populated");
            let total = lines.len();
            if total == 0 || rows == 0 {
                return Ok(None);
            }
            let top_logical = self.wrap.top_logical().min(total - 1);
            let lookahead = if self.wrap.soft_wrap() {
                rows.saturating_add(8)
            } else {
                rows
            };
            let end = top_logical.saturating_add(lookahead).min(total);
            return Ok(Some((lines[top_logical..end].to_vec(), top_logical, total)));
        }

        let total = self.line_source.total_lines();
        if total == 0 || rows == 0 {
            return Ok(None);
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
            return Ok(Some((Vec::new(), top_logical, total)));
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
        Ok(Some((styled, top_logical, total)))
    }

    /// Walk pre-styled visible lines through [`emit_visual_rows`]. The
    /// styled slice covers `[top_logical .. top_logical + styled.len())`.
    fn emit_window(
        &self,
        ctx: &RenderCtx,
        rows: usize,
        styled: &[String],
        top_logical: usize,
        total: usize,
    ) -> Vec<String> {
        let usable = self.usable_width(total);
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

    /// Move the current-match cursor by `delta` (wrapping at the ends)
    /// and scroll that match's line to the top of the viewport. No-op
    /// when no search is active or there are no matches.
    fn step_match(&mut self, delta: isize) {
        let line = self.search.as_mut().and_then(|s| s.step(delta));
        if let Some(line) = line {
            self.wrap.jump_to_line(line);
            self.reveal_match_h(line);
            self.clamp_top();
        }
    }

    /// After a search jump to `line`, position `h_scroll` so the match
    /// is on screen. With soft-wrap on, `h_scroll` is inert and resets
    /// to 0; with wrap off, pan minimally via `search::reveal_h_scroll`
    /// so an already-visible hit isn't disturbed.
    fn reveal_match_h(&mut self, line: usize) {
        if self.wrap.soft_wrap() {
            self.wrap.clear_h_scroll();
            return;
        }
        let usable = self.usable_width(self.current_total());
        let span = self
            .search
            .as_ref()
            .and_then(|s| s.line_overlay(line))
            .and_then(|(ranges, current)| {
                let r = ranges.get(current?)?;
                Some((r.start, r.end))
            });
        let h = match span {
            Some((start, end)) => search::reveal_h_scroll(self.wrap.h_scroll(), usable, start, end),
            None => 0,
        };
        self.wrap.set_h_scroll(h);
    }
}

impl Mode for ContentMode {
    fn id(&self) -> ModeId {
        ModeId::Content
    }

    fn label(&self) -> &str {
        self.label
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, rows: usize) -> Result<Window> {
        // Capture viewport geometry; `scroll` from the caller is ignored
        // because ContentMode owns its own scroll state. cached_cols and
        // cached_rows are read by `scroll()` (which has no ctx) and by
        // wrap math helpers that don't take a RenderCtx parameter.
        self.cached_cols = ctx.term_cols;
        self.cached_rows = rows;
        self.ensure_pretty_parsed();
        // `ensure_pretty_parsed` may have force-flipped to Raw on a
        // size-cap refusal; re-check readiness before branching.
        let pretty_ready = self.rendering.showing_pretty()
            && self.rendering.pretty().is_some_and(PrettyView::is_ready);
        let prepared = self.prepare_window(ctx, rows, pretty_ready)?;
        let window = match prepared {
            None => {
                // Empty output or rows == 0 — surface the active
                // output's total so the status line still tracks
                // document size.
                let total = if pretty_ready {
                    self.rendering
                        .pretty()
                        .and_then(PrettyView::rendered_lines)
                        .map(<[String]>::len)
                        .unwrap_or(0)
                } else {
                    self.line_source.total_lines()
                };
                Window {
                    lines: Vec::new(),
                    total,
                }
            }
            Some((styled, top_logical, total)) => {
                let lines = self.emit_window(ctx, rows, &styled, top_logical, total);
                Window { lines, total }
            }
        };
        // After the active branch is materialized (raw line count is
        // always known; pretty count becomes known on first render),
        // re-clamp top so a window resize / theme cycle that changed
        // wrap segment counts doesn't leave us scrolled past the bottom.
        self.clamp_top();
        Ok(window)
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        self.ensure_pretty_parsed();
        let pretty_text = if self.rendering.showing_pretty() {
            self.rendering.pretty().and_then(PrettyView::text)
        } else {
            None
        };
        super::content_pipe::render(
            ctx,
            out,
            pretty_text,
            &self.line_source,
            self.highlighter.as_mut(),
            self.syntax_token.as_deref(),
            &self.theme_manager,
            &self.gutter,
        )
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        if self.rendering.allow_toggle() {
            RAW_TOGGLE_ACTIONS
        } else {
            LINE_NUMBER_ACTIONS
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let mut segs: Vec<(String, Color)> = Vec::new();
        if self.rendering.allow_toggle() {
            // A failed pretty branch (size cap or parse error) locks the
            // user in raw — surface that so the inert `r` key isn't a
            // mystery.
            let label = if self.rendering.pretty().is_some_and(PrettyView::failed) {
                "Raw (forced)"
            } else if self.rendering.showing_pretty() {
                "Pretty"
            } else {
                "Raw"
            };
            segs.push((label.to_string(), theme.label));
        }
        // Surface wrap state only when on (default-on convention: the
        // segment's absence means "off"; matches color-mode segment
        // which only appears when changed off the default).
        if self.wrap.soft_wrap() {
            segs.push(("Wrap".to_string(), theme.muted));
        }
        // Search position, shown only while a search is active.
        if let Some(search) = &self.search {
            segs.push(search.status_segment(theme));
        }
        segs
    }

    fn handle(&mut self, action: Action) -> Handled {
        // Esc clears an active search before falling through to the
        // global Back (pop frame / quit) — matches less / vim. With no
        // search active, Back is left untouched.
        if action == Action::Back && self.search.is_some() {
            self.search = None;
            return Handled::Yes;
        }
        if action == Action::NextMatch {
            self.step_match(1);
            return Handled::Yes;
        }
        if action == Action::PrevMatch {
            self.step_match(-1);
            return Handled::Yes;
        }
        if action == Action::ToggleLineNumbers {
            self.gutter.toggle();
            return Handled::Yes;
        }
        if action == Action::ToggleSoftWrap {
            // Logical line stays put; sub-row / h-scroll reset so the
            // post-flip viewport is coherent — handled by `toggle_wrap`.
            self.wrap.toggle_wrap();
            return Handled::Yes;
        }
        if action == Action::ToggleRawSource && self.rendering.toggle() {
            // Pretty line N and raw line N are unrelated content — the
            // user's previous scroll offset would put them somewhere
            // arbitrary in the new view. Reset to the top. The
            // highlighter doesn't need an explicit reset here: its
            // `at()` is preserved across the toggle, and the next
            // raw-mode `render_window` will detect `at() > 0` (the new
            // scroll) and reset itself before catching up.
            if let Some(pv) = self.rendering.pretty_mut() {
                pv.invalidate_render();
            }
            // Match positions are in the old output's line domain —
            // they mean nothing in the new one. Drop the search.
            self.search = None;
            self.wrap.jump_to_top();
            self.wrap.clear_h_scroll();
            Handled::Yes
        } else {
            Handled::No
        }
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        let cl = active_view(&self.rendering, &self.line_source);
        if cl.total() == 0 {
            // No content yet — nothing to navigate. Still consume the
            // action so it doesn't fall through to a nonsensical global.
            return matches!(
                action,
                Action::ScrollUp
                    | Action::ScrollDown
                    | Action::PageUp
                    | Action::PageDown
                    | Action::Top
                    | Action::Bottom
                    | Action::ScrollLeft
                    | Action::ScrollRight
            );
        }
        let usable = self.usable_width(cl.total());
        let rows = self.cached_rows.max(1);
        match action {
            Action::ScrollUp => self.wrap.step_up(&cl, usable),
            Action::ScrollDown => self.wrap.step_down(&cl, usable),
            Action::PageUp => {
                for _ in 0..rows.saturating_sub(1).max(1) {
                    self.wrap.step_up(&cl, usable);
                }
            }
            Action::PageDown => {
                for _ in 0..rows.saturating_sub(1).max(1) {
                    self.wrap.step_down(&cl, usable);
                }
            }
            Action::Top => self.wrap.jump_to_top(),
            Action::Bottom => self.wrap.jump_to_bottom(&cl, usable, rows),
            Action::ScrollLeft => self.wrap.pan_left(),
            Action::ScrollRight => self.wrap.pan_right(),
            _ => return false,
        }
        self.wrap.clamp(&cl, usable, rows);
        true
    }

    fn rerender_on_resize(&self) -> bool {
        // Wrap segments and h-scroll slicing are width-dependent.
        true
    }

    fn on_resize(&mut self, term_cols: usize, term_rows: usize) {
        self.cached_cols = term_cols;
        self.cached_rows = term_rows;
        // A narrower terminal can leave us h-scrolled past content;
        // re-clamp via clamp_top + bound h_scroll loosely (let it ride
        // since horizontal "max" is cheap to recompute on the fly and
        // naturally bounded by line widths).
        self.clamp_top();
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }

    fn total_lines(&self) -> Option<usize> {
        // ViewerState's line-scroll math is suppressed for owns_scroll
        // modes, so this is no longer a load-bearing hint. Report the
        // raw line count when known (cheap via LineSource); skip in
        // pretty since the count needs the materialized cache that
        // appears only after the first render.
        if self.rendering.showing_pretty() {
            None
        } else {
            Some(self.line_source.total_lines())
        }
    }

    /// ContentMode tracks position in line units when showing raw —
    /// the line index then corresponds 1:1 to source lines, so a
    /// switch to Hex (and back) lands on the right byte.
    ///
    /// In pretty mode the line index has no relation to source bytes
    /// (e.g. pretty-printed JSON line 50 may correspond to source byte
    /// 200 or 20000). Tracking would lie, so we opt out: switching
    /// from pretty Content to Hex preserves whatever position Hex
    /// previously had instead of synthesizing a wrong one.
    fn tracks_position(&self) -> bool {
        !self.rendering.showing_pretty()
    }

    fn position(&self) -> Position {
        if self.rendering.showing_pretty() {
            Position::Unknown
        } else {
            Position::Line(self.wrap.top_logical())
        }
    }

    fn set_position(&mut self, pos: Position, source: &InputSource) {
        let line = match pos {
            Position::Line(l) => Some(l),
            Position::Byte(b) => source.byte_to_line(b),
            Position::Unknown => None,
        };
        if let Some(l) = line {
            self.wrap.jump_to_line(l);
            self.wrap.clear_h_scroll();
            self.clamp_top();
        }
    }

    /// Scan the active branch for `query`, jump to the first match, and
    /// arm match highlighting. `None` or an empty query clears the
    /// search. Smart-case: an all-lowercase query matches
    /// case-insensitively, any uppercase makes it case-sensitive.
    ///
    /// The scan is one full pass over the active branch — `LineSource`
    /// when raw, the pretty-printed string when pretty. `ContentMode`
    /// owns its scroll, so it positions itself on the first match and
    /// the returned line is unused by the caller.
    fn set_search(&mut self, query: Option<&str>) -> Option<usize> {
        let query = match query {
            Some(q) if !q.is_empty() => q,
            _ => {
                self.search = None;
                return None;
            }
        };
        let pretty_text = if self.rendering.showing_pretty() {
            self.rendering.pretty().and_then(PrettyView::text)
        } else {
            None
        };
        let search = if let Some(text) = pretty_text {
            SearchState::scan(text.lines(), query)
        } else {
            // `iter_all` yields `Result<String>`; a decode error becomes
            // an empty line so line indices stay aligned with the view.
            SearchState::scan(
                self.line_source.iter_all().map(|r| r.unwrap_or_default()),
                query,
            )
        };
        let first = search.first_line();
        self.search = Some(search);
        if let Some(line) = first {
            self.wrap.jump_to_line(line);
            self.reveal_match_h(line);
        }
        self.clamp_top();
        first
    }
}
