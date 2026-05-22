//! Aligned table view — a sticky-header table over fully-materialised
//! rows. Shared by object files (Sections / Symbols) and classfiles
//! (Fields / Methods).
//!
//! A dedicated mode rather than a `ContentMode` because it needs two
//! things `ContentMode` can't give plain text:
//!
//! * a **sticky header** — the column header + rule are re-emitted as
//!   the top two lines on every frame, so they stay pinned when the
//!   body scrolls past the viewport bottom;
//! * **live re-theming** — every cell is repainted from the frame's
//!   theme, so a runtime theme cycle recolours the whole table.
//!
//! Display-only: vertical scroll, Left/Right pan, `/` search. Rows
//! wider than the terminal are panned, never wrapped.

use anyhow::Result;
use syntect::highlighting::Color;
use unicode_width::UnicodeWidthStr;

use super::{Align, Cell, CellRole, Column, Table};
use crate::output::PrintOutput;
use crate::theme::{PeekTheme, lerp_color};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::viewer::search::{SearchState, overlay_matches, reveal_h_scroll};
use crate::viewer::ui::{Action, HelpEntry, slice_styled_h, take_cols};

/// Sticky rows at the top of the viewport — the header and its rule.
const STICKY_ROWS: usize = 2;

/// Columns moved per Left/Right keypress.
const H_STEP: usize = 8;

pub(crate) struct TableMode {
    label: &'static str,
    table: Table,
    /// Plain (unpainted) text of every body line — the notice (when
    /// present) followed by one line per row. Search scans this; the
    /// index lines up 1:1 with the painted body lines.
    body_plain: Vec<String>,
    /// Widest visible line (header or body) — bounds horizontal scroll.
    content_width: usize,
    /// Body-line index at the top of the scroll viewport.
    top: usize,
    /// Left-most visible column — horizontal scroll offset.
    h_scroll: usize,
    /// Terminal width / content-area height from the last render —
    /// scroll math reads them.
    cached_cols: usize,
    cached_rows: usize,
    search: Option<SearchState>,
}

const TABLE_ACTIONS: &[HelpEntry] = &[
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Pan left / right",
    ),
    (&[Action::OpenSearch], "Search"),
    (
        &[Action::NextMatch, Action::PrevMatch],
        "Next / previous match",
    ),
];

impl TableMode {
    pub(crate) fn new(label: &'static str, table: Table) -> Self {
        let mut body_plain = Vec::with_capacity(table.rows.len() + 1);
        if let Some(notice) = &table.notice {
            body_plain.push(notice.clone());
        }
        for row in &table.rows {
            body_plain.push(plain_row(row, &table.columns));
        }
        let content_width = body_plain
            .iter()
            .map(|l| l.chars().count())
            .chain(std::iter::once(header_width(&table.columns)))
            .max()
            .unwrap_or(0);
        Self {
            label,
            table,
            body_plain,
            content_width,
            top: 0,
            h_scroll: 0,
            cached_cols: 0,
            cached_rows: 0,
            search: None,
        }
    }

    /// Body rows that fit below the sticky header.
    fn body_rows(&self) -> usize {
        self.cached_rows.saturating_sub(STICKY_ROWS).max(1)
    }

    fn max_top(&self) -> usize {
        self.body_plain.len().saturating_sub(self.body_rows())
    }

    fn clamp_top(&mut self) {
        self.top = self.top.min(self.max_top());
    }

    fn clamp_h_scroll(&mut self) {
        self.h_scroll = self.h_scroll.min(self.content_width.saturating_sub(1));
    }

    /// Number of leading body lines occupied by the notice (0 or 1).
    fn notice_lines(&self) -> usize {
        usize::from(self.table.notice.is_some())
    }

    /// Paint body line `idx`, with any active search match overlaid.
    fn body_line(&self, idx: usize, theme: &PeekTheme) -> String {
        let base = if idx < self.notice_lines() {
            theme.paint(self.table.notice.as_deref().unwrap_or(""), theme.muted)
        } else {
            painted_row(
                &self.table.rows[idx - self.notice_lines()],
                &self.table.columns,
                theme,
            )
        };
        match self.search.as_ref().and_then(|s| s.line_overlay(idx)) {
            Some((ranges, current)) => overlay_matches(&base, &ranges, current, theme),
            None => base,
        }
    }

    /// Scroll body line `line` into view and pan horizontally to reveal
    /// the active search match — minimally, via `reveal_h_scroll`.
    fn reveal_match(&mut self, line: usize) {
        self.top = line;
        self.clamp_top();
        if let Some((start, end)) = self.match_span(line) {
            self.h_scroll = reveal_h_scroll(self.h_scroll, self.cached_cols, start, end);
            self.clamp_h_scroll();
        }
    }

    /// Column span `[start, end)` of the active search match on `line`.
    fn match_span(&self, line: usize) -> Option<(usize, usize)> {
        let (ranges, current) = self.search.as_ref()?.line_overlay(line)?;
        let r = ranges.get(current?)?;
        Some((r.start, r.end))
    }

    /// Step the search cursor and reveal the new match.
    fn step_match(&mut self, delta: isize) {
        if let Some(line) = self.search.as_mut().and_then(|s| s.step(delta)) {
            self.reveal_match(line);
        }
    }
}

impl Mode for TableMode {
    fn id(&self) -> ModeId {
        // Same id CsvTableMode uses — a tabular primary view. Nothing
        // keys on Content being unique (Tab cycles by index).
        ModeId::Content
    }

    fn label(&self) -> &str {
        self.label
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, rows: usize) -> Result<Window> {
        self.cached_rows = rows;
        self.cached_cols = ctx.term_cols;
        self.clamp_top();
        let theme = ctx.peek_theme;
        let cols = ctx.term_cols;
        let h = self.h_scroll;

        let mut lines: Vec<String> = Vec::with_capacity(rows);
        lines.push(clip(&paint_header(&self.table.columns, theme), h, cols));
        lines.push(clip(&paint_rule(self.content_width, theme), h, cols));

        let end = (self.top + self.body_rows()).min(self.body_plain.len());
        for idx in self.top..end {
            lines.push(clip(&self.body_line(idx, theme), h, cols));
        }
        Ok(Window {
            lines,
            total: self.body_plain.len(),
        })
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let theme = ctx.peek_theme;
        out.write_line(&paint_header(&self.table.columns, theme))?;
        out.write_line(&paint_rule(self.content_width, theme))?;
        for idx in 0..self.body_plain.len() {
            out.write_line(&self.body_line(idx, theme))?;
        }
        Ok(())
    }

    fn total_lines(&self) -> Option<usize> {
        Some(self.body_plain.len())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        let step = self.body_rows();
        match action {
            Action::ScrollUp => self.top = self.top.saturating_sub(1),
            Action::ScrollDown => {
                self.top = self.top.saturating_add(1);
                self.clamp_top();
            }
            Action::PageUp => self.top = self.top.saturating_sub(step),
            Action::PageDown => {
                self.top = self.top.saturating_add(step);
                self.clamp_top();
            }
            Action::Top => self.top = 0,
            Action::Bottom => self.top = self.max_top(),
            Action::ScrollLeft => self.h_scroll = self.h_scroll.saturating_sub(H_STEP),
            Action::ScrollRight => {
                self.h_scroll = self.h_scroll.saturating_add(H_STEP);
                self.clamp_h_scroll();
            }
            _ => return false,
        }
        true
    }

    fn rerender_on_resize(&self) -> bool {
        // Rows are clipped / panned to terminal width.
        true
    }

    fn on_resize(&mut self, term_cols: usize, term_rows: usize) {
        self.cached_cols = term_cols;
        self.cached_rows = term_rows;
        self.clamp_top();
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        TABLE_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::NextMatch => {
                self.step_match(1);
                Handled::Yes
            }
            Action::PrevMatch => {
                self.step_match(-1);
                Handled::Yes
            }
            Action::Back if self.search.is_some() => {
                self.search = None;
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let total = self.body_plain.len();
        let cur = (self.top + 1).min(total.max(1));
        let mut segs = vec![(format!("{cur}/{total}"), theme.muted)];
        // Horizontal pan offset — shown only when panned (default-state
        // convention: absence means h_scroll == 0).
        if self.h_scroll > 0 {
            segs.push((format!("\u{2192}{}", self.h_scroll), theme.muted));
        }
        if let Some(search) = &self.search {
            segs.push(search.status_segment(theme));
        }
        segs
    }

    fn set_search(&mut self, query: Option<&str>) -> Option<usize> {
        let query = match query {
            Some(q) if !q.is_empty() => q,
            _ => {
                self.search = None;
                return None;
            }
        };
        let search = SearchState::scan(self.body_plain.iter(), query);
        let first = search.first_line();
        self.search = Some(search);
        if let Some(line) = first {
            self.reveal_match(line);
        }
        first
    }
}

/// Slice a styled line to `cols` visible columns starting at `h_scroll`.
fn clip(line: &str, h_scroll: usize, cols: usize) -> String {
    slice_styled_h(line, h_scroll, cols)
}

/// Format a cell / header value into its column: truncate-and-pad for a
/// fixed column, verbatim for the flexible last column (`width == 0`).
fn fmt_plain(text: &str, col: &Column, is_last: bool) -> String {
    if is_last || col.width == 0 {
        return text.to_string();
    }
    let t = truncate(text, col.width);
    match col.align {
        Align::Left => format!("{t:<width$}", width = col.width),
        Align::Right => format!("{t:>width$}", width = col.width),
    }
}

/// Plain (unpainted) text of one row — used for search scanning and to
/// keep the painted line's visible width identical.
fn plain_row(row: &[Cell], columns: &[Column]) -> String {
    let last = columns.len().saturating_sub(1);
    row.iter()
        .enumerate()
        .map(|(i, c)| fmt_plain(&c.text, &columns[i], i == last))
        .collect::<Vec<_>>()
        .join("  ")
}

/// Painted row — same visible text as `plain_row`, each cell coloured by
/// its [`CellRole`] against `theme`.
fn painted_row(row: &[Cell], columns: &[Column], theme: &PeekTheme) -> String {
    let last = columns.len().saturating_sub(1);
    row.iter()
        .enumerate()
        .map(|(i, c)| {
            let padded = fmt_plain(&c.text, &columns[i], i == last);
            paint_cell(&padded, c.role, theme)
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn paint_cell(padded: &str, role: CellRole, theme: &PeekTheme) -> String {
    match role {
        CellRole::Address => paint_addr(padded, theme),
        CellRole::Tag => theme.paint(padded, theme.label),
        CellRole::Primary => theme.paint(padded, theme.accent),
        CellRole::Numeric => theme.paint(padded, lerp_color(theme.accent, theme.value, 0.5)),
        CellRole::Muted => theme.paint(padded, theme.muted),
        CellRole::Name => theme.paint(padded, theme.foreground),
    }
}

/// Paint a right-padded `0x…` address: leading pad and `0x` prefix dim,
/// hex digits in the value colour.
fn paint_addr(padded: &str, theme: &PeekTheme) -> String {
    match padded.find("0x") {
        Some(i) => {
            let (prefix, digits) = padded.split_at(i + 2);
            let mut s = theme.paint(prefix, theme.muted);
            s.push_str(&theme.paint(digits, theme.value));
            s
        }
        None => theme.paint(padded, theme.muted),
    }
}

/// Sticky header row — column names in the muted colour.
fn paint_header(columns: &[Column], theme: &PeekTheme) -> String {
    let last = columns.len().saturating_sub(1);
    columns
        .iter()
        .enumerate()
        .map(|(i, col)| theme.paint(&fmt_plain(col.header, col, i == last), theme.muted))
        .collect::<Vec<_>>()
        .join("  ")
}

/// Sticky rule under the header, spanning the table's full content width
/// (widest of header and body) so it never cuts short of a wide body cell
/// in the flexible last column.
fn paint_rule(width: usize, theme: &PeekTheme) -> String {
    theme.paint(&"\u{2500}".repeat(width), theme.muted)
}

/// Visible width of the header row (sum of column widths + separators).
fn header_width(columns: &[Column]) -> usize {
    let last = columns.len().saturating_sub(1);
    let mut width = 0usize;
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            width += 2; // column separator
        }
        width += if i == last || col.width == 0 {
            col.header.chars().count()
        } else {
            col.width
        };
    }
    width
}

/// Clamp a name to `max` display columns, ending with `…` when cut.
fn truncate(s: &str, max: usize) -> String {
    if UnicodeWidthStr::width(s) <= max {
        return s.to_string();
    }
    format!("{}\u{2026}", take_cols(s, max.saturating_sub(1)))
}
