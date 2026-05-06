use std::rc::Rc;

use anyhow::Result;
use syntect::highlighting::Color;

use super::{Handled, Mode, ModeId, Position, RenderCtx, Window};
use crate::input::detect::StructuredFormat;
use crate::input::{InputSource, LineSource};
use crate::output::PrintOutput;
use crate::theme::{ColorMode, PeekTheme, PeekThemeName, ThemeManager};
use crate::types::structured::pretty;
use crate::viewer::ui::{Action, count_wrap_segments, slice_styled_h, wrap_styled};
use crate::viewer::{LineStreamHighlighter, highlight_lines};

/// Horizontal-scroll step size (columns) when wrap is off. Matches
/// `less -S` feel: small enough to land naturally on indented code,
/// big enough that panning a wide log line doesn't take 20 keypresses.
const H_SCROLL_STEP: usize = 8;

/// Pretty-printing the structured form of a file requires holding the
/// whole document — there's no streaming JSON pretty-printer. Reads above
/// this size fall back to the raw streamed view with a warning so we
/// don't OOM on a multi-GB log shaped like JSON.
const PRETTY_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Content view: text, syntax-highlighted source, pretty-printed structured
/// data, or SVG XML source.
///
/// Raw mode streams from a `LineSource` (anchor-indexed line iterator
/// over the input). Each `render_window` fetches just the visible window
/// of lines; multi-GB files never load into memory. With a syntax token,
/// a forward-only `LineStreamHighlighter` is driven across the window;
/// backward scrolls past its current cursor reset and replay from line 0.
///
/// Pretty mode requires the full document and so caps file size at
/// `PRETTY_MAX_BYTES`. Above the cap we push a warning, force `use_pretty
/// = false`, and the user sees the raw (streamed) source. Below the cap
/// the parsed-and-pretty-printed text is cached on first access; if a
/// syntax token is set, the cached pretty form is also re-highlighted as
/// a single batch (bounded by the cap).
///
/// `r` flips `use_pretty` when `allow_pretty_toggle` is set — used for
/// structured files (JSON/YAML/TOML/XML) and SVG XML, where raw vs
/// pretty is a meaningful user choice. Source code / plain text have no
/// pretty form, so `r` is inert. The active sub-state (Pretty / Raw)
/// shows up as a status-line segment.
pub(crate) struct ContentMode {
    source: InputSource,
    line_source: LineSource,
    /// Forward-only syntect feeder for raw-mode highlighting. `None` when
    /// the view has no associated syntax (plain text, --plain mode).
    highlighter: Option<LineStreamHighlighter>,

    /// Format to pretty-print as, when one applies. None means "no pretty
    /// form available" (source code, plain text).
    pretty_target: Option<StructuredFormat>,
    /// Lazy pretty-print result. `None` = not yet attempted; `Some(Ok)` =
    /// cached pretty text; `Some(Err)` = parse failure (the matching
    /// warning was already pushed to `pending_warnings`).
    pretty: Option<Result<String, String>>,
    /// Highlighted-pretty cache, keyed by the (theme, color) tuple it was
    /// rendered for. Pretty content is bounded by `PRETTY_MAX_BYTES` so
    /// holding one materialized `Vec<String>` is acceptable. Invalidated
    /// whenever the active theme/color tuple changes.
    pretty_highlighted: Option<(PeekThemeName, ColorMode, Vec<String>)>,
    /// Cached split of pretty content into raw lines (no syntax). Same
    /// reasoning as `pretty_highlighted` — bounded by the cap.
    pretty_raw_lines: Option<Vec<String>>,

    /// Warnings produced during render that haven't been collected by
    /// `ViewerState` yet — drained on every `take_warnings` call.
    pending_warnings: Vec<String>,
    syntax_token: Option<String>,
    theme_manager: Rc<ThemeManager>,
    use_pretty: bool,
    allow_pretty_toggle: bool,
    show_line_numbers: bool,
    label: &'static str,

    /// Soft-wrap on by default. When on, vertical scroll moves visual
    /// rows (not logical lines) and the gutter blanks out continuation
    /// rows. When off, lines truncate at viewport width and Left/Right
    /// pan via `h_scroll`.
    soft_wrap: bool,
    /// Top visible logical line index. Owned scroll state — replaces
    /// `ViewerState::scroll[active]` for ContentMode.
    top_logical: usize,
    /// Visual-row offset inside `top_logical`'s wrap segments. Used only
    /// when `soft_wrap` is on; reset on toggle, position restore, and
    /// after Top jumps.
    top_sub_row: usize,
    /// Horizontal column offset when `soft_wrap` is off.
    h_scroll: usize,
    /// Last terminal column count seen — set on every render and via
    /// `on_resize`. `scroll()` reads this rather than querying the
    /// terminal directly. (HexMode follows the same pattern.)
    cached_cols: usize,
    /// Last terminal row count seen (content area height).
    cached_rows: usize,
}

const RAW_TOGGLE_ACTIONS: &[(Action, &str)] = &[
    (Action::ToggleRawSource, "Toggle raw / pretty"),
    (Action::ToggleLineNumbers, "Toggle line numbers"),
    (Action::ToggleSoftWrap, "Toggle soft wrap"),
    (Action::ScrollLeft, "Pan left (wrap off)"),
    (Action::ScrollRight, "Pan right (wrap off)"),
];

const LINE_NUMBER_ACTIONS: &[(Action, &str)] = &[
    (Action::ToggleLineNumbers, "Toggle line numbers"),
    (Action::ToggleSoftWrap, "Toggle soft wrap"),
    (Action::ScrollLeft, "Pan left (wrap off)"),
    (Action::ScrollRight, "Pan right (wrap off)"),
];

impl ContentMode {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        source: InputSource,
        line_source: LineSource,
        pretty_target: Option<StructuredFormat>,
        syntax_token: Option<String>,
        theme_manager: Rc<ThemeManager>,
        initial_theme: PeekThemeName,
        initial_use_pretty: bool,
        allow_pretty_toggle: bool,
        show_line_numbers: bool,
        label: &'static str,
    ) -> Self {
        let highlighter = syntax_token.as_ref().map(|t| {
            LineStreamHighlighter::new(t.clone(), Rc::clone(&theme_manager), initial_theme)
        });
        Self {
            source,
            line_source,
            highlighter,
            pretty_target,
            pretty: None,
            pretty_highlighted: None,
            pretty_raw_lines: None,
            pending_warnings: Vec::new(),
            syntax_token,
            theme_manager,
            use_pretty: initial_use_pretty && pretty_target.is_some(),
            allow_pretty_toggle,
            show_line_numbers,
            label,
            soft_wrap: true,
            top_logical: 0,
            top_sub_row: 0,
            h_scroll: 0,
            cached_cols: 0,
            cached_rows: 0,
        }
    }

    /// Number of digits in `n`, minimum 2 (so a 9-line file's gutter
    /// doesn't bounce in width as you scroll past line 9 vs line 99).
    fn gutter_digit_width(total: usize) -> usize {
        let mut digits = 1;
        let mut n = total;
        while n >= 10 {
            n /= 10;
            digits += 1;
        }
        digits.max(2)
    }

    /// Mutate `lines` in place, prepending a right-aligned line-number
    /// gutter painted in the theme's gutter color. `start` is the
    /// 0-based line index of `lines[0]` in the full source; `total` is
    /// the source's total line count, used to size the gutter so the
    /// width is stable across the visible window.
    ///
    /// Used by the print/pipe path (`render_to_pipe`) only — interactive
    /// rendering uses `gutter_prefix` per visual row so wrap continuation
    /// rows can blank the gutter.
    fn apply_gutter(lines: &mut [String], start: usize, total: usize, peek_theme: &PeekTheme) {
        if total == 0 || lines.is_empty() {
            return;
        }
        let width = Self::gutter_digit_width(total);
        let color_mode = peek_theme.color_mode;
        let fg_open = color_mode.fg_seq(peek_theme.gutter);
        let reset = color_mode.reset();
        for (offset, line) in lines.iter_mut().enumerate() {
            let n = start + offset + 1;
            let gutter = format!("{fg_open}{n:>width$} │ {reset}");
            let mut prefixed = String::with_capacity(gutter.len() + line.len());
            prefixed.push_str(&gutter);
            prefixed.push_str(line);
            *line = prefixed;
        }
    }

    /// Visible-cell width of the line-number gutter, including the
    /// trailing " │ " separator. Zero when line numbers are off or the
    /// source is empty. The same width is reserved on continuation rows
    /// (gutter blanked) so wrapped text aligns under its first segment.
    fn gutter_visible_width(&self, total: usize) -> usize {
        if self.show_line_numbers && total > 0 {
            Self::gutter_digit_width(total) + 3
        } else {
            0
        }
    }

    /// Build the gutter prefix for one visual row. `line_num = Some(n)`
    /// for the first wrap segment of a logical line; `None` for
    /// continuation rows (gutter blanked but width preserved). Returns
    /// an empty string when the gutter is disabled.
    fn gutter_prefix(
        &self,
        line_num: Option<usize>,
        total: usize,
        peek_theme: &PeekTheme,
    ) -> String {
        if !self.show_line_numbers || total == 0 {
            return String::new();
        }
        let width = Self::gutter_digit_width(total);
        let color_mode = peek_theme.color_mode;
        let fg = color_mode.fg_seq(peek_theme.gutter);
        let reset = color_mode.reset();
        match line_num {
            Some(n) => format!("{fg}{n:>width$} │ {reset}"),
            None => format!("{fg}{:>width$} │ {reset}", ""),
        }
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
        let line_num = line_idx + 1;
        if !self.soft_wrap {
            let body = slice_styled_h(styled, self.h_scroll, usable_width);
            let prefix = self.gutter_prefix(Some(line_num), total, peek_theme);
            out.push(format!("{prefix}{body}"));
            return out.len() >= max_rows;
        }
        let segments = wrap_styled(styled, usable_width);
        for (seg_idx, seg) in segments.iter().enumerate() {
            if seg_idx < first_skip {
                continue;
            }
            let prefix = if seg_idx == 0 {
                self.gutter_prefix(Some(line_num), total, peek_theme)
            } else {
                self.gutter_prefix(None, total, peek_theme)
            };
            out.push(format!("{prefix}{seg}"));
            if out.len() >= max_rows {
                return true;
            }
        }
        false
    }

    /// Total logical line count of the currently-active branch. Raw
    /// branch reads from `LineSource` (cheap); pretty branch reads from
    /// the materialized cache (only valid after first render). Returns
    /// 0 when the pretty cache hasn't been built yet (scroll handler
    /// runs after render so this case is unusual but defensive).
    fn current_total(&self) -> usize {
        if self.use_pretty {
            if let Some((_, _, lines)) = &self.pretty_highlighted {
                return lines.len();
            }
            if let Some(lines) = &self.pretty_raw_lines {
                return lines.len();
            }
            0
        } else {
            self.line_source.total_lines()
        }
    }

    /// Number of wrap segments for the logical line at `idx` given the
    /// usable visual width. Returns 1 when wrap is off (each logical
    /// line is one visual row) or when the line text isn't reachable.
    fn segment_count_at(&self, idx: usize, usable: usize) -> usize {
        if !self.soft_wrap || usable == 0 {
            return 1;
        }
        if self.use_pretty {
            if let Some((_, _, lines)) = &self.pretty_highlighted
                && let Some(l) = lines.get(idx)
            {
                return count_wrap_segments(l, usable);
            }
            if let Some(lines) = &self.pretty_raw_lines
                && let Some(l) = lines.get(idx)
            {
                return count_wrap_segments(l, usable);
            }
            1
        } else {
            self.line_source
                .window(idx..idx + 1)
                .ok()
                .and_then(|v| v.into_iter().next())
                .map(|l| count_wrap_segments(&l, usable))
                .unwrap_or(1)
        }
    }

    /// Walk backward from EOF accumulating segment counts until at least
    /// `rows` visual rows are below the candidate top. Returns
    /// `(top_logical, top_sub_row)` — the position that places the file
    /// end exactly at the bottom of the viewport (or the file start when
    /// the document is shorter than the viewport).
    fn bottom_position(&self, total: usize, usable: usize, rows: usize) -> (usize, usize) {
        if total == 0 || rows == 0 {
            return (0, 0);
        }
        if !self.soft_wrap {
            return (total.saturating_sub(rows), 0);
        }
        let mut accum = 0usize;
        let mut idx = total;
        while idx > 0 {
            idx -= 1;
            let segs = self.segment_count_at(idx, usable);
            accum += segs;
            if accum >= rows {
                return (idx, accum - rows);
            }
        }
        (0, 0)
    }

    /// Re-clamp the current top position so it never sits past the
    /// effective bottom. Called after every scroll mutation so
    /// `top_logical` / `top_sub_row` stay valid even when the user
    /// pages aggressively or the viewport just shrank.
    fn clamp_top(&mut self) {
        let total = self.current_total();
        if total == 0 {
            self.top_logical = 0;
            self.top_sub_row = 0;
            return;
        }
        let cols = self.cached_cols.max(1);
        let gutter_w = self.gutter_visible_width(total);
        let usable = cols.saturating_sub(gutter_w).max(1);
        let rows = self.cached_rows.max(1);
        let (max_l, max_s) = self.bottom_position(total, usable, rows);
        if self.top_logical > max_l {
            self.top_logical = max_l;
            self.top_sub_row = max_s;
        } else if self.top_logical == max_l && self.top_sub_row > max_s {
            self.top_sub_row = max_s;
        }
        if !self.soft_wrap {
            self.top_sub_row = 0;
        }
    }

    /// Advance `(top_logical, top_sub_row)` by one visual row downward.
    /// In wrap-on, walks segments within the current line then rolls
    /// over to the next line. In wrap-off, just bumps `top_logical`.
    fn step_visual_down(&mut self) {
        let total = self.current_total();
        if total == 0 {
            return;
        }
        if !self.soft_wrap {
            self.top_logical = self.top_logical.saturating_add(1);
            return;
        }
        let cols = self.cached_cols.max(1);
        let gutter_w = self.gutter_visible_width(total);
        let usable = cols.saturating_sub(gutter_w).max(1);
        let segs = self.segment_count_at(self.top_logical, usable);
        if self.top_sub_row + 1 < segs {
            self.top_sub_row += 1;
        } else {
            self.top_logical = self.top_logical.saturating_add(1);
            self.top_sub_row = 0;
        }
    }

    /// Step `(top_logical, top_sub_row)` upward by one visual row.
    fn step_visual_up(&mut self) {
        let total = self.current_total();
        if total == 0 {
            return;
        }
        if !self.soft_wrap {
            self.top_logical = self.top_logical.saturating_sub(1);
            return;
        }
        if self.top_sub_row > 0 {
            self.top_sub_row -= 1;
            return;
        }
        if self.top_logical == 0 {
            return;
        }
        self.top_logical -= 1;
        let cols = self.cached_cols.max(1);
        let gutter_w = self.gutter_visible_width(total);
        let usable = cols.saturating_sub(gutter_w).max(1);
        let segs = self.segment_count_at(self.top_logical, usable);
        self.top_sub_row = segs.saturating_sub(1);
    }

    /// Run pretty-print if it's the first time pretty mode is rendered.
    /// Caches the result; on parse failure pushes one warning. Above the
    /// size cap, refuses to load: pushes a warning and forces `use_pretty
    /// = false` so the streamed raw view takes over.
    fn ensure_pretty(&mut self) {
        if self.pretty.is_some() {
            return;
        }
        let Some(target) = self.pretty_target else {
            return;
        };
        if self.line_source.total_bytes() > PRETTY_MAX_BYTES {
            let mb = self.line_source.total_bytes() / (1024 * 1024);
            self.pending_warnings.push(format!(
                "file too large for pretty-print ({mb} MB > {} MB cap); showing raw source",
                PRETTY_MAX_BYTES / (1024 * 1024)
            ));
            // Surface a sentinel error so `render_window` falls through
            // to the raw branch; also flip `use_pretty` so subsequent
            // renders skip the cap check entirely.
            self.pretty = Some(Err("size cap".to_string()));
            self.use_pretty = false;
            return;
        }
        let raw = match self.source.read_text() {
            Ok(s) => s,
            Err(e) => {
                self.pending_warnings.push(format!(
                    "read failed for pretty-print ({e}); showing raw source"
                ));
                self.pretty = Some(Err(e.to_string()));
                return;
            }
        };
        self.pretty = Some(match pretty::pretty_print(&raw, target) {
            Ok(s) => Ok(s),
            Err(e) => {
                let format_name = match target {
                    StructuredFormat::Json => "JSON",
                    StructuredFormat::Yaml => "YAML",
                    StructuredFormat::Toml => "TOML",
                    StructuredFormat::Xml => "XML",
                };
                self.pending_warnings.push(format!(
                    "{format_name} parse failed ({e}); showing raw source"
                ));
                Err(e.to_string())
            }
        });
    }

    /// Pretty branch: ensure caches are populated for the active theme/
    /// color, then walk visible logical lines and feed them through
    /// `emit_visual_rows` until `rows` visual rows are produced.
    ///
    /// Caller has guaranteed `self.pretty` is `Some(Ok(_))`.
    fn render_pretty_window(&mut self, ctx: &RenderCtx, rows: usize) -> Result<Window> {
        if self.syntax_token.is_some() {
            let stale = self
                .pretty_highlighted
                .as_ref()
                .is_none_or(|(t, c, _)| *t != ctx.theme_name || *c != ctx.peek_theme.color_mode);
            if stale {
                let highlighted = {
                    let pretty = match &self.pretty {
                        Some(Ok(s)) => s.as_str(),
                        _ => unreachable!("render_window guards pretty branch"),
                    };
                    let token = self.syntax_token.as_deref().unwrap();
                    highlight_lines(
                        pretty,
                        token,
                        &self.theme_manager,
                        ctx.theme_name,
                        ctx.peek_theme.color_mode,
                    )?
                };
                self.pretty_highlighted =
                    Some((ctx.theme_name, ctx.peek_theme.color_mode, highlighted));
            }
        } else if self.pretty_raw_lines.is_none() {
            let pretty = match &self.pretty {
                Some(Ok(s)) => s.as_str(),
                _ => unreachable!("render_window guards pretty branch"),
            };
            self.pretty_raw_lines = Some(pretty.lines().map(String::from).collect());
        }
        let lines: &[String] = if self.syntax_token.is_some() {
            &self.pretty_highlighted.as_ref().unwrap().2
        } else {
            self.pretty_raw_lines.as_ref().unwrap()
        };
        let total = lines.len();
        if total == 0 || rows == 0 {
            return Ok(Window {
                lines: Vec::new(),
                total,
            });
        }

        let cols = self.cached_cols.max(1);
        let gutter_w = self.gutter_visible_width(total);
        let usable = cols.saturating_sub(gutter_w).max(1);
        let top_logical = self.top_logical.min(total - 1);
        let mut first_skip = if self.soft_wrap { self.top_sub_row } else { 0 };

        let lookahead = if self.soft_wrap {
            rows.saturating_add(8)
        } else {
            rows
        };
        let end = top_logical.saturating_add(lookahead).min(total);

        let mut emitted: Vec<String> = Vec::with_capacity(rows);
        for (line_idx, styled) in lines.iter().enumerate().take(end).skip(top_logical) {
            let stop = self.emit_visual_rows(
                &mut emitted,
                rows,
                line_idx,
                styled,
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
        Ok(Window {
            lines: emitted,
            total,
        })
    }

    /// Raw streaming branch: catch the highlighter up to `top_logical`
    /// (re-feeding throwaway lines) then feed visible logical lines and
    /// produce visual rows via `emit_visual_rows`. Backward scroll past
    /// the highlighter's cursor triggers a reset+replay.
    fn render_raw_window(&mut self, ctx: &RenderCtx, rows: usize) -> Result<Window> {
        let total = self.line_source.total_lines();
        if total == 0 || rows == 0 {
            return Ok(Window {
                lines: Vec::new(),
                total,
            });
        }

        let cols = self.cached_cols.max(1);
        let gutter_w = self.gutter_visible_width(total);
        let usable = cols.saturating_sub(gutter_w).max(1);
        let top_logical = self.top_logical.min(total - 1);
        let mut first_skip = if self.soft_wrap { self.top_sub_row } else { 0 };

        if let Some(hl) = self.highlighter.as_mut() {
            let theme_changed = hl.active_theme() != ctx.theme_name;
            if theme_changed || hl.at() > top_logical {
                hl.reset(ctx.theme_name);
            }
        }

        let start_at = self.highlighter.as_ref().map_or(top_logical, |h| h.at());
        // Lookahead buffer: each visible logical line yields ≥ 1 visual
        // row so `rows` lines is enough; the small margin absorbs cases
        // where `first_skip` swallows leading segments of the top line.
        let lookahead = if self.soft_wrap {
            rows.saturating_add(8)
        } else {
            rows
        };
        let end_at = top_logical.saturating_add(lookahead).min(total);
        if start_at >= end_at {
            return Ok(Window {
                lines: Vec::new(),
                total,
            });
        }
        let raw_lines = self.line_source.window(start_at..end_at)?;
        let mut emitted: Vec<String> = Vec::with_capacity(rows);
        for (offset, raw) in raw_lines.iter().enumerate() {
            let line_idx = start_at + offset;
            let styled = if let Some(hl) = self.highlighter.as_mut() {
                hl.feed(raw, ctx.peek_theme.color_mode)?
            } else {
                raw.clone()
            };
            if line_idx < top_logical {
                continue;
            }
            let stop = self.emit_visual_rows(
                &mut emitted,
                rows,
                line_idx,
                &styled,
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
        Ok(Window {
            lines: emitted,
            total,
        })
    }

    /// Drop both pretty caches. Called when the active branch flips so
    /// the new branch starts fresh and doesn't carry stale memory.
    fn invalidate_pretty_caches(&mut self) {
        self.pretty_highlighted = None;
        self.pretty_raw_lines = None;
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
        if self.use_pretty {
            self.ensure_pretty();
        }
        // After ensure_pretty, `use_pretty` may have been forced off due
        // to the size cap; re-check before branching.
        let pretty_ready = self.use_pretty && matches!(self.pretty, Some(Ok(_)));
        let result = if pretty_ready {
            self.render_pretty_window(ctx, rows)
        } else {
            self.render_raw_window(ctx, rows)
        };
        // After the active branch is materialized (raw line count is
        // always known; pretty count becomes known on first render),
        // re-clamp top so a window resize / theme cycle that changed
        // wrap segment counts doesn't leave us scrolled past the bottom.
        self.clamp_top();
        result
    }

    /// Pipe-mode render. Raw streams line-by-line through the highlighter
    /// (or unstyled `line_source.iter_all()`); pretty writes the full
    /// pretty string in one shot — same byte-fidelity as before A1 for
    /// un-highlighted text (no synthetic trailing newline added).
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        if self.use_pretty {
            self.ensure_pretty();
        }
        if self.use_pretty
            && let Some(Ok(pretty)) = self.pretty.as_ref()
        {
            if let Some(ref token) = self.syntax_token {
                let mut lines = highlight_lines(
                    pretty,
                    token,
                    &self.theme_manager,
                    ctx.theme_name,
                    ctx.peek_theme.color_mode,
                )?;
                if self.show_line_numbers {
                    let total = lines.len();
                    Self::apply_gutter(&mut lines, 0, total, ctx.peek_theme);
                }
                for line in &lines {
                    out.write_line(line)?;
                }
            } else if self.show_line_numbers {
                let mut lines: Vec<String> = pretty.lines().map(String::from).collect();
                let total = lines.len();
                Self::apply_gutter(&mut lines, 0, total, ctx.peek_theme);
                for line in &lines {
                    out.write_line(line)?;
                }
            } else {
                out.write_str(pretty)?;
            }
            return Ok(());
        }
        // Pretty unavailable — fall through to raw stream.

        // Raw stream. With a syntax token, every line (including the
        // last) is `\n`-terminated — pre-A1 contract: escape sequences
        // are line-scoped and the natural shape is per-line writes.
        // Without a token, preserve the source's trailing-newline status
        // for byte-for-byte fidelity (matches `cat` and the pre-A1
        // un-highlighted path).
        let total = self.line_source.total_lines();
        let gutter_width = if self.show_line_numbers && total > 0 {
            Some(Self::gutter_digit_width(total))
        } else {
            None
        };
        let color_mode = ctx.peek_theme.color_mode;
        let gutter_fg = color_mode.fg_seq(ctx.peek_theme.gutter);
        let gutter_reset = color_mode.reset();
        let prefix = |n: usize| -> Option<String> {
            gutter_width.map(|w| format!("{gutter_fg}{n:>w$} │ {gutter_reset}"))
        };

        if let Some(hl) = self.highlighter.as_mut() {
            hl.reset(ctx.theme_name);
            for (idx, line) in self.line_source.iter_all().enumerate() {
                let line = line?;
                let escaped = hl.feed(&line, color_mode)?;
                if let Some(p) = prefix(idx + 1) {
                    out.write_line(&format!("{p}{escaped}"))?;
                } else {
                    out.write_line(&escaped)?;
                }
            }
        } else {
            let trailing_nl = self.line_source.ends_with_newline();
            for (idx, line) in self.line_source.iter_all().enumerate() {
                let line = line?;
                let is_last = idx + 1 == total;
                let body = if let Some(p) = prefix(idx + 1) {
                    format!("{p}{line}")
                } else {
                    line
                };
                if is_last && !trailing_nl {
                    out.write_str(&body)?;
                } else {
                    out.write_line(&body)?;
                }
            }
        }
        Ok(())
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        if self.allow_pretty_toggle {
            RAW_TOGGLE_ACTIONS
        } else {
            LINE_NUMBER_ACTIONS
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let mut segs: Vec<(String, Color)> = Vec::new();
        if self.allow_pretty_toggle {
            // `pretty: Some(Err)` means pretty was attempted and refused
            // (size cap or parse failure). `use_pretty` is forced false in
            // that case, so the user is effectively locked in raw — surface
            // that explicitly so the inert `r` key isn't mysterious.
            let label = match (&self.pretty, self.use_pretty) {
                (Some(Err(_)), _) => "Raw (forced)",
                (_, true) => "Pretty",
                (_, false) => "Raw",
            };
            segs.push((label.to_string(), theme.label));
        }
        // Surface wrap state only when on (default-on convention: the
        // segment's absence means "off"; matches color-mode segment
        // which only appears when changed off the default).
        if self.soft_wrap {
            segs.push(("Wrap".to_string(), theme.muted));
        }
        segs
    }

    fn handle(&mut self, action: Action) -> Handled {
        if action == Action::ToggleLineNumbers {
            self.show_line_numbers = !self.show_line_numbers;
            return Handled::Yes;
        }
        if action == Action::ToggleSoftWrap {
            self.soft_wrap = !self.soft_wrap;
            // Logical line stays put; only sub-row / h-scroll need
            // resetting so post-flip the viewport is coherent.
            self.top_sub_row = 0;
            self.h_scroll = 0;
            return Handled::Yes;
        }
        if action == Action::ToggleRawSource
            && self.allow_pretty_toggle
            && self.pretty_target.is_some()
            // Pretty-cap fallback is permanent for this session: pretty was
            // attempted and refused (size cap or parse error) so flipping
            // `use_pretty` would be invisible (next render falls through to
            // raw anyway) and the scroll-reset would surprise the user.
            && !matches!(self.pretty, Some(Err(_)))
        {
            self.use_pretty = !self.use_pretty;
            // Pretty line N and raw line N are unrelated content — the
            // user's previous scroll offset would put them somewhere
            // arbitrary in the new view. Reset to the top. The
            // highlighter doesn't need an explicit reset here: its
            // `at()` is preserved across the toggle, and the next
            // raw-mode `render_window` will detect `at() > 0` (the new
            // scroll) and reset itself before catching up.
            self.invalidate_pretty_caches();
            self.top_logical = 0;
            self.top_sub_row = 0;
            self.h_scroll = 0;
            Handled::Yes
        } else {
            Handled::No
        }
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        let total = self.current_total();
        if total == 0 {
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
        let cols = self.cached_cols.max(1);
        let gutter_w = self.gutter_visible_width(total);
        let usable = cols.saturating_sub(gutter_w).max(1);
        let rows = self.cached_rows.max(1);
        match action {
            Action::ScrollUp => {
                self.step_visual_up();
            }
            Action::ScrollDown => {
                self.step_visual_down();
            }
            Action::PageUp => {
                let step = rows.saturating_sub(1).max(1);
                for _ in 0..step {
                    self.step_visual_up();
                }
            }
            Action::PageDown => {
                let step = rows.saturating_sub(1).max(1);
                for _ in 0..step {
                    self.step_visual_down();
                }
            }
            Action::Top => {
                self.top_logical = 0;
                self.top_sub_row = 0;
            }
            Action::Bottom => {
                let (l, s) = self.bottom_position(total, usable, rows);
                self.top_logical = l;
                self.top_sub_row = s;
            }
            Action::ScrollLeft => {
                if !self.soft_wrap {
                    self.h_scroll = self.h_scroll.saturating_sub(H_SCROLL_STEP);
                }
            }
            Action::ScrollRight => {
                if !self.soft_wrap {
                    self.h_scroll = self.h_scroll.saturating_add(H_SCROLL_STEP);
                }
            }
            _ => return false,
        }
        self.clamp_top();
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
        if self.use_pretty {
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
        !self.use_pretty
    }

    fn position(&self) -> Position {
        if self.use_pretty {
            Position::Unknown
        } else {
            Position::Line(self.top_logical)
        }
    }

    fn set_position(&mut self, pos: Position, source: &InputSource) {
        let line = match pos {
            Position::Line(l) => Some(l),
            Position::Byte(b) => source.byte_to_line(b),
            Position::Unknown => None,
        };
        if let Some(l) = line {
            self.top_logical = l;
            self.top_sub_row = 0;
            self.h_scroll = 0;
            self.clamp_top();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::RenderOptions;
    use crate::input::detect;
    use crate::theme::{PeekTheme, PeekThemeName};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn fixture(name: &str) -> InputSource {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("test-data");
        path.push(name);
        InputSource::File(path)
    }

    fn make_ctx<'a>(
        source: &'a InputSource,
        detected: &'a detect::Detected,
        file_info: &'a crate::info::FileInfo,
        peek_theme: &'a PeekTheme,
    ) -> RenderCtx<'a> {
        RenderCtx {
            source,
            detected,
            file_info,
            theme_name: PeekThemeName::IdeaDark,
            peek_theme,
            render_opts: RenderOptions::default(),
            term_cols: 80,
            term_rows: 24,
        }
    }

    /// End-to-end: ContentMode's streaming windowed render must match the
    /// whole-file `highlight_lines` output for the same line indices.
    /// Uses a real Rust fixture so the test goes through the full
    /// LineSource → LineStreamHighlighter → ranges_to_escaped path.
    #[test]
    fn render_window_matches_whole_file_highlight() {
        let source = fixture("theme.rs");
        let detected = detect::detect(&source).unwrap();
        let file_info = crate::info::gather(&source, &detected).unwrap();
        let tm = Rc::new(ThemeManager::new(
            PeekThemeName::IdeaDark,
            ColorMode::TrueColor,
        ));
        let peek_theme = tm.peek_theme().clone();

        let line_source = source.open_line_source().unwrap();
        let total = line_source.total_lines();
        assert!(total > 50, "fixture should have plenty of lines");

        // Reference: whole-file highlight via the same path the pre-A1
        // code used.
        let raw = source.read_text().unwrap();
        let whole = crate::viewer::highlight_lines(
            &raw,
            "rs",
            &tm,
            PeekThemeName::IdeaDark,
            ColorMode::TrueColor,
        )
        .unwrap();

        let mut mode = ContentMode::new(
            source.clone(),
            line_source,
            None,
            Some("rs".to_string()),
            Rc::clone(&tm),
            PeekThemeName::IdeaDark,
            false,
            false,
            false,
            "Source",
        );

        let ctx = make_ctx(&source, &detected, &file_info, &peek_theme);

        // Forward scroll: window 0..10 then 10..20 (incremental, no reset).
        // ContentMode owns scroll, so the `scroll` argument to
        // `render_window` is ignored — drive the position via the public
        // field directly. The fixture's longest line (76 cols) fits the
        // 80-col make_ctx width, so soft-wrap doesn't fragment lines and
        // the visual-row output equals the whole-file highlight 1:1.
        let w0 = mode.render_window(&ctx, 0, 10).unwrap();
        assert_eq!(w0.lines.len(), 10);
        assert_eq!(w0.total, total);
        for (i, line) in w0.lines.iter().enumerate() {
            assert_eq!(line, &whole[i], "forward window 0..10 line {i} drift");
        }

        mode.top_logical = 10;
        let w1 = mode.render_window(&ctx, 0, 10).unwrap();
        assert_eq!(w1.lines.len(), 10);
        for (i, line) in w1.lines.iter().enumerate() {
            assert_eq!(line, &whole[10 + i], "forward window 10..20 line {i} drift");
        }

        // Backward jump triggers a highlighter reset; output must still
        // match (this is the regression-prone path — wrong reset and
        // multi-line block-comment highlighting goes sideways).
        mode.top_logical = 0;
        let w_back = mode.render_window(&ctx, 0, 5).unwrap();
        for (i, line) in w_back.lines.iter().enumerate() {
            assert_eq!(line, &whole[i], "backward jump line {i} drift");
        }
    }

    /// Above the size cap, `ensure_pretty` should refuse to load, push a
    /// warning, and clear `use_pretty` so the user sees the streamed raw
    /// view instead.
    #[test]
    fn pretty_cap_falls_back_to_raw_with_warning() {
        // Pad past PRETTY_MAX_BYTES (16 MB) with valid JSON.
        let mut buf = String::with_capacity(PRETTY_MAX_BYTES as usize + 1024);
        buf.push('[');
        let entry = "0,";
        while (buf.len() as u64) < PRETTY_MAX_BYTES + 64 {
            buf.push_str(entry);
        }
        buf.pop(); // strip trailing comma
        buf.push(']');

        let source = InputSource::Stdin {
            data: Arc::from(buf.into_bytes().into_boxed_slice()),
        };
        let line_source = source.open_line_source().unwrap();
        assert!(line_source.total_bytes() > PRETTY_MAX_BYTES);

        let tm = Rc::new(ThemeManager::new(
            PeekThemeName::IdeaDark,
            ColorMode::TrueColor,
        ));

        let mut mode = ContentMode::new(
            source,
            line_source,
            Some(StructuredFormat::Json),
            Some("JSON".to_string()),
            tm,
            PeekThemeName::IdeaDark,
            true,  // initial_use_pretty
            true,  // allow_pretty_toggle
            false, // show_line_numbers
            "Content",
        );

        // Trigger the cap check via ensure_pretty directly.
        mode.ensure_pretty();
        assert!(!mode.use_pretty, "size cap must clear use_pretty");
        let warnings = mode.take_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("too large for pretty-print")),
            "expected size-cap warning, got {warnings:?}"
        );
    }

    /// Build a plain-text ContentMode with no syntax token from inline
    /// stdin bytes. Used by the wrap / h-scroll unit tests below — a
    /// minimal fixture so the visual-row math is the only moving part.
    fn plain_mode_from_bytes(bytes: &[u8]) -> ContentMode {
        let source = InputSource::Stdin {
            data: Arc::from(bytes.to_vec().into_boxed_slice()),
        };
        let line_source = source.open_line_source().unwrap();
        let tm = Rc::new(ThemeManager::new(PeekThemeName::IdeaDark, ColorMode::Plain));
        ContentMode::new(
            source,
            line_source,
            None, // no pretty target
            None, // no syntax token
            tm,
            PeekThemeName::IdeaDark,
            false, // initial_use_pretty
            false, // allow_pretty_toggle
            false, // show_line_numbers
            "Source",
        )
    }

    /// Wrap-on ScrollDown walks visual rows: advance the sub-row inside
    /// the current logical line, then roll over to the next line. With a
    /// 1-row viewport and `usable=10`, line 0 (20 cols) has 2 segments
    /// and line 1 has 1 segment, so the bottom is `(1, 0)`.
    #[test]
    fn wrap_on_scrolldown_advances_sub_row_then_logical() {
        let mut mode = plain_mode_from_bytes(b"AAAAAAAAAAAAAAAAAAAA\nBBBB\n");
        mode.cached_cols = 10;
        mode.cached_rows = 1;
        assert!(mode.soft_wrap, "default-on");
        assert_eq!((mode.top_logical, mode.top_sub_row), (0, 0));

        assert!(mode.scroll(Action::ScrollDown));
        assert_eq!((mode.top_logical, mode.top_sub_row), (0, 1));

        assert!(mode.scroll(Action::ScrollDown));
        assert_eq!((mode.top_logical, mode.top_sub_row), (1, 0));

        // Past bottom — clamp_top pins us to the bottom position.
        assert!(mode.scroll(Action::ScrollDown));
        assert_eq!((mode.top_logical, mode.top_sub_row), (1, 0));
    }

    /// Wrap-on ScrollUp from `(N, 0)` lands on the *last* segment of
    /// line N-1, not its segment 0.
    #[test]
    fn wrap_on_scrollup_lands_on_last_segment_of_previous_line() {
        let mut mode = plain_mode_from_bytes(b"AAAAAAAAAAAAAAAAAAAA\nBBBB\n");
        mode.cached_cols = 10;
        mode.cached_rows = 1;
        mode.top_logical = 1;
        mode.top_sub_row = 0;

        assert!(mode.scroll(Action::ScrollUp));
        // line 0 has 2 segments → last segment index is 1.
        assert_eq!((mode.top_logical, mode.top_sub_row), (0, 1));

        assert!(mode.scroll(Action::ScrollUp));
        assert_eq!((mode.top_logical, mode.top_sub_row), (0, 0));

        // Already at top — saturate.
        assert!(mode.scroll(Action::ScrollUp));
        assert_eq!((mode.top_logical, mode.top_sub_row), (0, 0));
    }

    /// Wrap-off ScrollRight steps `h_scroll` by `H_SCROLL_STEP` (8 cols)
    /// per press; ScrollLeft saturates at zero. Wrap-on Left/Right are
    /// inert (covered by exercising ScrollRight while soft_wrap=true).
    #[test]
    fn wrap_off_scrollright_steps_h_scroll_by_eight() {
        let mut mode = plain_mode_from_bytes(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n");
        mode.cached_cols = 80;
        mode.cached_rows = 5;
        mode.soft_wrap = false;

        assert_eq!(mode.h_scroll, 0);
        assert!(mode.scroll(Action::ScrollRight));
        assert_eq!(mode.h_scroll, 8);
        assert!(mode.scroll(Action::ScrollRight));
        assert_eq!(mode.h_scroll, 16);
        assert!(mode.scroll(Action::ScrollLeft));
        assert_eq!(mode.h_scroll, 8);

        for _ in 0..5 {
            mode.scroll(Action::ScrollLeft);
        }
        assert_eq!(mode.h_scroll, 0);
    }

    /// Wrap-on Left/Right do not move `h_scroll` — h-scroll is only
    /// meaningful when wrap is off.
    #[test]
    fn wrap_on_left_right_do_not_move_h_scroll() {
        let mut mode = plain_mode_from_bytes(b"AAAAAAAAAAAAAAAAAAAA\n");
        mode.cached_cols = 10;
        mode.cached_rows = 5;
        assert!(mode.soft_wrap);

        mode.scroll(Action::ScrollRight);
        mode.scroll(Action::ScrollRight);
        assert_eq!(mode.h_scroll, 0);
    }

    /// `ToggleSoftWrap` flips wrap, resets `top_sub_row` and `h_scroll`,
    /// preserves `top_logical`. Coherent post-flip viewport.
    #[test]
    fn toggle_soft_wrap_resets_sub_row_and_h_scroll_preserves_logical() {
        let mut mode = plain_mode_from_bytes(b"AAAAAAAAAAAAAAAAAAAA\nBBBB\n");
        mode.cached_cols = 10;
        mode.cached_rows = 5;
        mode.top_logical = 1;
        mode.top_sub_row = 0;
        mode.soft_wrap = false;
        mode.h_scroll = 16;

        let r = mode.handle(Action::ToggleSoftWrap);
        assert_eq!(r, Handled::Yes);
        assert!(mode.soft_wrap);
        assert_eq!(mode.top_logical, 1);
        assert_eq!(mode.top_sub_row, 0);
        assert_eq!(mode.h_scroll, 0);

        // Flip back: top_logical stays, sub-row + h-scroll already 0.
        let r = mode.handle(Action::ToggleSoftWrap);
        assert_eq!(r, Handled::Yes);
        assert!(!mode.soft_wrap);
        assert_eq!(mode.top_logical, 1);
    }

    /// `status_segments` emits a `Wrap` segment when wrap is on and
    /// nothing extra when off (default-non-default convention).
    #[test]
    fn status_segments_show_wrap_only_when_on() {
        let mode = plain_mode_from_bytes(b"hi\n");
        let tm = ThemeManager::new(PeekThemeName::IdeaDark, ColorMode::Plain);
        let theme = tm.peek_theme().clone();
        // Default-on.
        let segs = mode.status_segments(&theme);
        assert!(segs.iter().any(|(s, _)| s == "Wrap"));

        let mut mode_off = plain_mode_from_bytes(b"hi\n");
        mode_off.soft_wrap = false;
        let segs = mode_off.status_segments(&theme);
        assert!(!segs.iter().any(|(s, _)| s == "Wrap"));
    }
}
