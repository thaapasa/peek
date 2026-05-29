//! Aligned, streaming table view used by CSV / TSV and the SQLite
//! contents viewer.
//!
//! Rendered shape:
//!
//! ```text
//!  name   │ age │ city
//!  ───────┼─────┼──────────
//!  alice  │ 30  │ Helsinki
//!  bob    │ 25  │ Tampere
//! ```
//!
//! State:
//!
//! * a [`RowSource`] backing — owns the row cache + pull-on-demand
//!   semantics (CSV's `csv::Reader`, SQLite's sliding-window cursor)
//! * `widths` — monotonic per-column widths, seeded from the first
//!   1000 rows and grown (never shrunk) as wider cells scroll into view
//! * `has_header` — runtime toggle (`Shift+H`), starts from the
//!   caller-provided seed
//! * `top_record` — record index at the top of the body viewport
//! * `h_col` — left-most visible column (column-step horizontal pan)
//!
//! Print mode renders the seeded widths only — no auto-widen — so the
//! table layout never depends on the deepest row consumed.
//!
//! **Not a [`super::TableMode`] subclass despite the shared shape.**
//! `TableMode` assumes a fully-materialised row list with fixed column
//! widths. This mode grows widths as rows stream in and consumes the
//! body lazily — different invariants, different state. Visual
//! similarity is intentional.

use std::borrow::Cow;
use std::ops::Range;

use anyhow::Result;
use syntect::highlighting::Color;
use unicode_width::UnicodeWidthStr;

use crate::output::PrintOutput;
use crate::theme::PeekTheme;
use crate::viewer::modes::{Handled, Mode, ModeId, NEXT_PREV_MATCH_HELP, RenderCtx, Window};
use crate::viewer::search::{
    MAX_MATCHES, SearchTarget, find_matches, overlay_matches, smart_case_sensitive,
};
use crate::viewer::table::row_source::RowSource;
use crate::viewer::ui::{Action, HelpEntry, take_cols};

/// One space of padding on each side of the column separator and on the
/// leading/trailing edges. Matches `column_sep` below.
const COL_SEP: &str = " │ ";
/// Glyph for separator-row segments under a column.
const SEP_ROW_CHAR: char = '─';
/// Junction glyph at column boundaries on the separator row.
const SEP_JUNCTION_CHAR: char = '┼';
/// Truncation marker on cells wider than their column width.
const TRUNCATE_MARKER: char = '…';
/// Placeholder for a `NULL` body cell (`Some(None)` from the source).
/// Painted muted by [`body_cell`] so it reads distinctly from both the
/// empty string and the literal text `"NULL"` (which paints in the
/// normal foreground color).
const NULL_MARKER: &str = "NULL";

/// Hard ceiling on per-column width — keeps a single huge cell from
/// pushing the whole table off the screen. Cells past this width are
/// truncated with [`TRUNCATE_MARKER`].
const MAX_COLUMN_WIDTH: usize = 64;

/// Horizontal alignment of a column's body cells. Decided up front by
/// the source's compose path (CSV: type inference from the seed body;
/// SQLite: declared column types).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Alignment {
    Left,
    Right,
}

pub(crate) struct RowsTableMode {
    source: Box<dyn RowSource>,
    /// Per-column widths. Monotonic — auto-widen grows them, `Shift+R`
    /// recomputes from the visible window. Length matches the column
    /// count of the source.
    widths: Vec<usize>,
    /// Seed widths captured at open time. Print-mode rendering uses
    /// these directly; interactive rendering may grow `widths` past them.
    seed_widths: Vec<usize>,
    /// Per-column horizontal alignment, fixed at construction so a
    /// header toggle doesn't shift numeric data to text.
    align: Vec<Alignment>,
    /// Runtime override of the caller-provided header seed. `Shift+H`
    /// toggles.
    has_header: bool,
    top_record: usize,
    h_col: usize,
    cached_cols: usize,
    cached_rows: usize,
    label: &'static str,
    /// Active cell-scoped search, or `None`. Cleared by `Back` / empty query.
    search: Option<CellSearch>,
}

/// Match-position cache for an active cell-scoped search. Each
/// [`CellMatch`] covers a single occurrence inside one cell; multi-
/// occurrence cells produce multiple entries with the same
/// `(record_idx, col_idx)` and different byte ranges. Ranges are byte
/// offsets into the cell's *display* form (post [`display_cell`]) so
/// they line up with `overlay_matches`.
struct CellSearch {
    matches: Vec<CellMatch>,
    /// Active-match index into `matches`. Unused when `matches` is empty.
    cursor: usize,
}

#[derive(Clone)]
struct CellMatch {
    record_idx: usize,
    col_idx: usize,
    /// Byte range inside the cell's display form.
    range: Range<usize>,
}

const TABLE_ACTIONS: &[HelpEntry] = &[
    (&[Action::ToggleHeader], "Toggle header row"),
    (&[Action::ReflowWidths], "Reflow column widths"),
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Pan columns left / right",
    ),
    (&[Action::OpenSearch], "Search cells"),
    NEXT_PREV_MATCH_HELP,
];

impl RowsTableMode {
    /// Build a row-table mode over the given source. `align` and
    /// `has_header` come from the source's compose path — they're
    /// decided per file type, not derived inside this generic mode.
    pub(crate) fn new(
        source: Box<dyn RowSource>,
        align: Vec<Alignment>,
        has_header: bool,
        label: &'static str,
    ) -> Self {
        let widths = seed_widths(&*source);
        Self {
            seed_widths: widths.clone(),
            widths,
            align,
            has_header,
            source,
            top_record: 0,
            h_col: 0,
            cached_cols: 0,
            cached_rows: 0,
            label,
            search: None,
        }
    }

    fn body_start(&self) -> usize {
        if self.has_header { 1 } else { 0 }
    }

    /// Total body records (excludes the header row when `has_header`).
    fn body_total(&self) -> usize {
        let loaded = self.source.loaded();
        loaded.saturating_sub(self.body_start())
    }

    /// Highest valid `top_record` for the current viewport. Accounts for
    /// the sticky header + separator rows (which occupy 2 visual rows
    /// when a header is shown).
    fn max_top(&self) -> usize {
        let reserved = if self.has_header { 2 } else { 0 };
        let rows = self.cached_rows.saturating_sub(reserved).max(1);
        self.body_total().saturating_sub(rows)
    }

    fn clamp_top(&mut self) {
        let max = self.max_top();
        if self.top_record > max {
            self.top_record = max;
        }
    }

    fn clamp_h_col(&mut self) {
        let cols = self.widths.len();
        if cols == 0 {
            self.h_col = 0;
            return;
        }
        if self.h_col >= cols {
            self.h_col = cols - 1;
        }
    }

    /// Reflow widths from the records currently visible in the viewport.
    /// Header row participates so header text doesn't get truncated.
    fn reflow_visible(&mut self) {
        let cols = self.widths.len();
        if cols == 0 {
            return;
        }
        let mut new_widths = vec![0usize; cols];
        if self.has_header {
            grow_widths_from_row(&*self.source, 0, &mut new_widths, cols);
        }
        let reserved = if self.has_header { 2 } else { 0 };
        let rows = self.cached_rows.saturating_sub(reserved);
        let start = self.body_start() + self.top_record;
        let end = (start + rows).min(self.source.loaded());
        for idx in start..end {
            grow_widths_from_row(&*self.source, idx, &mut new_widths, cols);
        }
        // Don't drop below a single column-character — empty columns
        // would render zero-width and merge into their neighbour separator.
        for w in &mut new_widths {
            if *w == 0 {
                *w = 1;
            }
        }
        self.widths = new_widths;
    }

    /// Walk newly-visible records and grow `widths` to fit any wider
    /// cells. Header row participates so the header text never narrows
    /// below its rendered form.
    fn grow_widths_for_visible(&mut self) {
        let cols = self.widths.len();
        if cols == 0 {
            return;
        }
        let reserved = if self.has_header { 2 } else { 0 };
        let rows = self.cached_rows.saturating_sub(reserved);
        let start = self.body_start() + self.top_record;
        let end = (start + rows).min(self.source.loaded());
        for idx in start..end {
            grow_widths_from_row(&*self.source, idx, &mut self.widths, cols);
        }
        if self.has_header {
            grow_widths_from_row(&*self.source, 0, &mut self.widths, cols);
        }
    }

    /// Match ranges (in display-form bytes) for one cell, plus which of
    /// them is the active cursor match. Empty when no search is active
    /// or the cell has no matches.
    fn cell_match_ranges(
        &self,
        record_idx: usize,
        col_idx: usize,
    ) -> (Vec<Range<usize>>, Option<usize>) {
        let Some(s) = &self.search else {
            return (Vec::new(), None);
        };
        if s.matches.is_empty() {
            return (Vec::new(), None);
        }
        let mut ranges = Vec::new();
        let mut current: Option<usize> = None;
        for (i, m) in s.matches.iter().enumerate() {
            if m.record_idx == record_idx && m.col_idx == col_idx {
                if i == s.cursor {
                    current = Some(ranges.len());
                }
                ranges.push(m.range.clone());
            }
        }
        (ranges, current)
    }

    /// Build a [`CellSearch`] from the loaded records. Drives the source
    /// to EOF first so search is exhaustive — the user expects a search
    /// to span the whole file.
    fn build_search(&mut self, query: &str) -> CellSearch {
        let _ = self.source.ensure_all();
        let sensitive = smart_case_sensitive(query);
        let mut matches: Vec<CellMatch> = Vec::new();
        let cols = self.widths.len();
        let loaded = self.source.loaded();
        'records: for record_idx in 0..loaded {
            if self.source.row_is_malformed(record_idx) {
                continue;
            }
            let Some(cells) = self.source.row(record_idx) else {
                continue;
            };
            for (col_idx, cell) in cells.iter().enumerate().take(cols) {
                let raw = cell.as_deref().unwrap_or("");
                let display = display_cell(raw);
                for r in find_matches(&display, query, sensitive) {
                    matches.push(CellMatch {
                        record_idx,
                        col_idx,
                        range: r,
                    });
                    if matches.len() >= MAX_MATCHES {
                        break 'records;
                    }
                }
            }
        }
        CellSearch { matches, cursor: 0 }
    }

    /// Step the search cursor by `delta`, wrapping at both ends, and
    /// scroll the new match into view.
    fn step_match(&mut self, delta: isize) {
        let Some(s) = self.search.as_mut() else {
            return;
        };
        let n = s.matches.len();
        if n == 0 {
            return;
        }
        let cur = s.cursor as isize;
        let next = ((cur + delta).rem_euclid(n as isize)) as usize;
        s.cursor = next;
        self.scroll_to_current_match();
    }

    /// Bring the current match's cell into view: scroll vertically and
    /// pan horizontally. The header row is always visible, so a match
    /// in record 0 (when `has_header` is on) just pans columns.
    fn scroll_to_current_match(&mut self) {
        let Some(s) = &self.search else { return };
        if s.matches.is_empty() {
            return;
        }
        let m = &s.matches[s.cursor];
        let record_idx = m.record_idx;
        let col_idx = m.col_idx;
        if !self.has_header || record_idx >= self.body_start() {
            let body_idx = record_idx.saturating_sub(self.body_start());
            self.top_record = body_idx;
            self.clamp_top();
        }
        if col_idx < self.widths.len() {
            self.h_col = col_idx;
            self.clamp_h_col();
        }
    }

    /// Build the header row painted with `theme.heading`. Returns
    /// `String::new()` when there's no header (the caller should skip
    /// emitting it).
    fn build_header_row(&self, widths: &[usize], theme: &PeekTheme) -> String {
        if !self.has_header {
            return String::new();
        }
        let Some(cells) = self.source.row(0) else {
            return String::new();
        };
        let mut out = String::new();
        out.push(' ');
        for (i, w) in widths.iter().enumerate().skip(self.h_col) {
            if i > self.h_col {
                out.push_str(&theme.paint_muted(COL_SEP));
            }
            let cell = cells.get(i).and_then(|c| c.as_deref()).unwrap_or("");
            let align = self.align.get(i).copied().unwrap_or(Alignment::Left);
            let (ranges, current) = self.cell_match_ranges(0, i);
            let painted = render_cell(cell, *w, theme.heading, align, theme, &ranges, current);
            out.push_str(&painted);
        }
        out
    }

    /// Build the separator row between header and body.
    fn build_separator_row(&self, widths: &[usize], theme: &PeekTheme) -> String {
        build_separator_row(widths, theme, self.h_col)
    }

    /// Build one body row at record index `body_idx` (0 = first body row).
    /// `malformed` flag paints the row with `theme.warning`.
    fn build_body_row(
        &self,
        body_idx: usize,
        widths: &[usize],
        theme: &PeekTheme,
    ) -> Option<String> {
        let rec_idx = self.body_start() + body_idx;
        let malformed = self.source.row_is_malformed(rec_idx);
        let cells = self.source.row(rec_idx)?;
        let mut out = String::new();
        out.push(' ');
        for (i, w) in widths.iter().enumerate().skip(self.h_col) {
            if i > self.h_col {
                out.push_str(&theme.paint_muted(COL_SEP));
            }
            let (cell, color): (&str, Color) = if malformed {
                ("<error>", theme.warning)
            } else {
                body_cell(cells, i, theme.foreground, theme)
            };
            let align = self.align.get(i).copied().unwrap_or(Alignment::Left);
            let (ranges, current) = self.cell_match_ranges(rec_idx, i);
            out.push_str(&render_cell(
                cell, *w, color, align, theme, &ranges, current,
            ));
        }
        Some(out)
    }
}

/// Walk one row of `source` and grow `widths` to fit each cell. Skips
/// malformed rows and missing rows. Shared between seed-width
/// construction, the auto-widen-on-scroll pass, and the manual reflow.
fn grow_widths_from_row(source: &dyn RowSource, idx: usize, widths: &mut [usize], cols: usize) {
    if source.row_is_malformed(idx) {
        return;
    }
    let Some(cells) = source.row(idx) else {
        return;
    };
    for (i, cell) in cells.iter().enumerate().take(cols) {
        // A present-but-NULL cell (`None`) reserves the `NULL` marker's
        // width so the marker isn't truncated when it renders. Cells
        // absent from the row entirely (ragged rows) never reach here —
        // they're past the slice end.
        let s = cell.as_deref().unwrap_or(NULL_MARKER);
        let w = display_cell(s).width().min(MAX_COLUMN_WIDTH);
        if w > widths[i] {
            widths[i] = w;
        }
    }
}

/// Render text + paint color for one body cell, distinguishing the three
/// states `cells.get(i)` can return:
///
/// * `Some(Some(v))` — a real value, painted with `base`.
/// * `Some(None)` — a SQL NULL, rendered as the muted [`NULL_MARKER`] so
///   it reads distinctly from the empty string and from a literal
///   `"NULL"` value (which stays `base`-colored).
/// * `None` — column absent from this row (a ragged row); rendered as
///   the empty string in `base`.
fn body_cell<'a>(
    cells: &'a [Option<String>],
    i: usize,
    base: Color,
    theme: &PeekTheme,
) -> (&'a str, Color) {
    match cells.get(i) {
        Some(Some(v)) => (v.as_str(), base),
        Some(None) => (NULL_MARKER, theme.muted),
        None => ("", base),
    }
}

/// Render one cell into its column. Truncates wider content with
/// [`TRUNCATE_MARKER`] and pads narrower content according to `align`.
/// Sanitises embedded newlines / tabs via [`display_cell`]; the inserted
/// `↵` marker is repainted with `theme.muted` so it reads as
/// non-content. Padding sits outside the colored span.
///
/// `match_ranges` are byte offsets into the cell's *display* form, used
/// to overlay search-match backgrounds. `current_idx` picks the active
/// match within `match_ranges` (cursor) — receives the brighter
/// background. Ranges that fall in the truncated tail are dropped.
fn render_cell(
    cell: &str,
    width: usize,
    color: Color,
    align: Alignment,
    theme: &PeekTheme,
    match_ranges: &[Range<usize>],
    current_idx: Option<usize>,
) -> String {
    let display = display_cell(cell);
    let cell_w = display.width();
    let (content, prefix_len, pad): (Cow<str>, usize, usize) = if cell_w > width {
        let t = take_cols(&display, width.saturating_sub(1));
        let prefix = t.len();
        let mut c = t;
        c.push(TRUNCATE_MARKER);
        (Cow::Owned(c), prefix, 0)
    } else {
        let len = display.len();
        (display, len, width - cell_w)
    };

    let mut inner = String::with_capacity(content.len() + 24);
    paint_content_with_markers(&mut inner, &content, color, theme);

    if !match_ranges.is_empty() {
        let (kept, kept_current) = filter_ranges_for_prefix(match_ranges, current_idx, prefix_len);
        if !kept.is_empty() {
            inner = overlay_matches(&inner, &kept, kept_current, theme);
        }
    }

    let mut out = String::with_capacity(inner.len() + pad);
    if matches!(align, Alignment::Right) {
        for _ in 0..pad {
            out.push(' ');
        }
    }
    out.push_str(&inner);
    if matches!(align, Alignment::Left) {
        for _ in 0..pad {
            out.push(' ');
        }
    }
    out
}

/// Drop ranges whose end exceeds `prefix_len` (i.e. fall inside the
/// truncation-replaced tail). Adjusts `current` if it pointed at a
/// dropped range — clears it. Kept ranges keep their original offsets
/// because the surviving prefix bytes are identical.
fn filter_ranges_for_prefix(
    ranges: &[Range<usize>],
    current: Option<usize>,
    prefix_len: usize,
) -> (Vec<Range<usize>>, Option<usize>) {
    let mut kept: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    let mut kept_current: Option<usize> = None;
    for (i, r) in ranges.iter().enumerate() {
        if r.end <= prefix_len {
            if current == Some(i) {
                kept_current = Some(kept.len());
            }
            kept.push(r.clone());
        }
    }
    (kept, kept_current)
}

/// Walk `content` char by char, painting `↵` markers with `theme.muted`
/// and everything else with `base`. Emits one final reset. No-op-fast
/// when there are no markers — single fg span + reset, identical to
/// `theme.paint(content, base)`.
fn paint_content_with_markers(out: &mut String, content: &str, base: Color, theme: &PeekTheme) {
    let style_mode = theme.style_mode;
    let mut current_is_marker = false;
    let mut span_open = false;
    for c in content.chars() {
        let want_marker = c == '\u{21B5}';
        if !span_open || want_marker != current_is_marker {
            let color = if want_marker { theme.muted } else { base };
            out.push_str(&style_mode.fg_seq(color));
            span_open = true;
            current_is_marker = want_marker;
        }
        out.push(c);
    }
    if span_open {
        out.push_str(style_mode.reset());
    }
}

/// Sanitize a cell's content for single-row display. Embedded newlines
/// would break the terminal cursor (pushing subsequent columns onto the
/// next visual row); tabs would expand to 8 cells unpredictably. The
/// table view is one-record-per-row, so we collapse:
///
/// * `\n` → `↵` (visible line-break marker, width 1)
/// * `\r` → drop (terminal would interpret as cursor-to-column-0)
/// * `\t` → space (tab stops aren't aligned across cells)
///
/// Returns `Cow::Borrowed` when the cell carries none of these — the
/// common case — so the hot path doesn't allocate.
pub(crate) fn display_cell(s: &str) -> Cow<'_, str> {
    if !s.contains(['\n', '\r', '\t']) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push('\u{21B5}'),
            '\r' => {}
            '\t' => out.push(' '),
            _ => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Build the initial per-column widths from the seed scan. Header cells
/// participate so the header text fits when present.
fn seed_widths(source: &dyn RowSource) -> Vec<usize> {
    let cols = source.column_count();
    if cols == 0 {
        return Vec::new();
    }
    let mut widths = vec![1usize; cols];
    let loaded = source.loaded();
    for idx in 0..loaded {
        grow_widths_from_row(source, idx, &mut widths, cols);
    }
    widths
}

impl Mode for RowsTableMode {
    fn id(&self) -> ModeId {
        ModeId::Content
    }

    fn label(&self) -> &str {
        self.label
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, rows: usize) -> Result<Window> {
        self.cached_cols = ctx.term_cols;
        self.cached_rows = rows;

        let reserved = if self.has_header { 2 } else { 0 };
        let body_rows = rows.saturating_sub(reserved);
        let body_target = self.body_start() + self.top_record + body_rows;
        // Pull enough records to fill the viewport (cheap when already
        // loaded; pulls from the source when not).
        let _ = self.source.ensure_row(body_target.saturating_sub(1));

        self.clamp_top();
        self.clamp_h_col();
        self.grow_widths_for_visible();

        let widths = self.widths.clone();
        let mut lines: Vec<String> = Vec::with_capacity(rows);

        if self.has_header {
            let header_row = self.build_header_row(&widths, ctx.peek_theme);
            if !header_row.is_empty() {
                lines.push(header_row);
            }
            lines.push(self.build_separator_row(&widths, ctx.peek_theme));
        }

        let total_body = self.body_total();
        let mut emitted = 0;
        let mut body_idx = self.top_record;
        while emitted < body_rows && body_idx < total_body {
            if let Some(row) = self.build_body_row(body_idx, &widths, ctx.peek_theme) {
                lines.push(row);
            }
            body_idx += 1;
            emitted += 1;
        }

        // `total` drives scroll math elsewhere; report total body records
        // (matches the user-visible scroll domain — every body record is
        // one logical row).
        Ok(Window {
            lines,
            total: total_body,
        })
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        // Drive the source to its end so every record renders.
        let _ = self.source.ensure_all();
        // Print mode uses seed widths only — single-row overflow allowed
        // (alignment breaks for that row, next row realigns).
        let widths = self.seed_widths.clone();
        if self.has_header && self.source.loaded() > 0 {
            out.write_line(&self.build_header_row_print(&widths, ctx.peek_theme))?;
            out.write_line(&self.build_separator_row_print(&widths, ctx.peek_theme))?;
        }
        let body_start = self.body_start();
        let loaded = self.source.loaded();
        for rec_idx in body_start..loaded {
            let malformed = self.source.row_is_malformed(rec_idx);
            let mut row = String::new();
            row.push(' ');
            if malformed {
                row.push_str(
                    &ctx.peek_theme
                        .paint("<malformed record>", ctx.peek_theme.warning),
                );
                out.write_line(&row)?;
                continue;
            }
            let Some(cells) = self.source.row(rec_idx) else {
                continue;
            };
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    row.push_str(&ctx.peek_theme.paint_muted(COL_SEP));
                }
                let (raw, color) = body_cell(cells, i, ctx.peek_theme.foreground, ctx.peek_theme);
                let cell = display_cell(raw);
                let cell_w = cell.width();
                let align = self.align.get(i).copied().unwrap_or(Alignment::Left);
                if cell_w > *w {
                    // Print-mode overflow: emit cell in full, push the rest
                    // of this row rightward past terminal edge. Re-paint
                    // the marker glyph muted in line with the interactive
                    // path so a multi-line cell prints consistently.
                    let mut painted = String::new();
                    paint_content_with_markers(&mut painted, &cell, color, ctx.peek_theme);
                    row.push_str(&painted);
                } else {
                    row.push_str(&render_cell(
                        &cell,
                        *w,
                        color,
                        align,
                        ctx.peek_theme,
                        &[],
                        None,
                    ));
                }
            }
            out.write_line(&row)?;
        }
        Ok(())
    }

    fn total_lines(&self) -> Option<usize> {
        Some(self.body_total())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        match action {
            Action::ScrollUp => {
                self.top_record = self.top_record.saturating_sub(1);
                true
            }
            Action::ScrollDown => {
                self.top_record = self.top_record.saturating_add(1);
                self.clamp_top();
                true
            }
            Action::PageUp => {
                let reserved = if self.has_header { 2 } else { 0 };
                let step = self.cached_rows.saturating_sub(reserved).max(1);
                self.top_record = self.top_record.saturating_sub(step);
                true
            }
            Action::PageDown => {
                let reserved = if self.has_header { 2 } else { 0 };
                let step = self.cached_rows.saturating_sub(reserved).max(1);
                self.top_record = self.top_record.saturating_add(step);
                // Try to pull records before clamping so Bottom-ish jumps
                // surface every loadable row.
                let body_target = self.body_start() + self.top_record + step;
                let _ = self.source.ensure_row(body_target.saturating_sub(1));
                self.clamp_top();
                true
            }
            Action::Top => {
                self.top_record = 0;
                true
            }
            Action::Bottom => {
                // Drive to EOF so the bottom is a true bottom.
                let _ = self.source.ensure_all();
                self.top_record = self.max_top();
                true
            }
            Action::ScrollLeft => {
                self.h_col = self.h_col.saturating_sub(1);
                true
            }
            Action::ScrollRight => {
                if self.h_col + 1 < self.widths.len() {
                    self.h_col += 1;
                }
                true
            }
            _ => false,
        }
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        TABLE_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::ReflowWidths => {
                self.reflow_visible();
                Handled::Yes
            }
            Action::ToggleHeader => {
                self.has_header = !self.has_header;
                // top_record stays in body-domain so the same body record
                // remains at the top; auto-widen will pick up the header
                // cell on the next render.
                self.clamp_top();
                Handled::Yes
            }
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

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn on_resize(&mut self, term_cols: usize, term_rows: usize) {
        self.cached_cols = term_cols;
        self.cached_rows = term_rows;
        self.clamp_top();
        self.clamp_h_col();
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let mut segs: Vec<(String, Color)> = Vec::new();
        // Records: `cur/total` body rows (or `cur/≥loaded` while partial).
        let body_total = self.body_total();
        let cur = self.top_record.saturating_add(1).min(body_total.max(1));
        let total_label = match self.source.total() {
            Some(_) => body_total.to_string(),
            None => format!("≥{body_total}"),
        };
        segs.push((format!("{cur}/{total_label}"), theme.muted));
        // Column count + h_col offset.
        let cols = self.widths.len();
        if cols > 0 {
            segs.push((format!("col {}/{}", self.h_col + 1, cols), theme.muted));
        }
        // Surface malformed counter only when non-zero (status-bar
        // minimalism convention).
        let malformed = self.source.malformed_count();
        if malformed > 0 {
            segs.push((format!("malformed {malformed}"), theme.warning));
        }
        // Header-on default; surface only when the user has flipped it off.
        if !self.has_header {
            segs.push(("Header off".to_string(), theme.label));
        }
        // Search position, shown only while a search is active.
        if let Some(s) = &self.search {
            let label = if s.matches.is_empty() {
                "no match".to_string()
            } else {
                format!("{}/{}", s.cursor + 1, s.matches.len())
            };
            segs.push((label, theme.label));
        }
        segs
    }

    fn set_search(&mut self, query: Option<&str>) -> SearchTarget {
        let query = match query {
            Some(q) if !q.is_empty() => q,
            _ => {
                self.search = None;
                return SearchTarget::Owned;
            }
        };
        let search = self.build_search(query);
        self.search = Some(search);
        self.scroll_to_current_match();
        SearchTarget::Owned
    }
}

impl RowsTableMode {
    /// Header row variant for print mode — uses plain widths without
    /// honoring `h_col` (print mode emits every column from index 0).
    fn build_header_row_print(&self, widths: &[usize], theme: &PeekTheme) -> String {
        let Some(cells) = self.source.row(0) else {
            return String::new();
        };
        let mut out = String::new();
        out.push(' ');
        for (i, w) in widths.iter().enumerate() {
            if i > 0 {
                out.push_str(&theme.paint_muted(COL_SEP));
            }
            let raw = cells.get(i).and_then(|c| c.as_deref()).unwrap_or("");
            let cell = display_cell(raw);
            let cell_w = cell.width();
            let align = self.align.get(i).copied().unwrap_or(Alignment::Left);
            if cell_w > *w {
                let mut painted = String::new();
                paint_content_with_markers(&mut painted, &cell, theme.heading, theme);
                out.push_str(&painted);
            } else {
                out.push_str(&render_cell(
                    &cell,
                    *w,
                    theme.heading,
                    align,
                    theme,
                    &[],
                    None,
                ));
            }
        }
        out
    }

    fn build_separator_row_print(&self, widths: &[usize], theme: &PeekTheme) -> String {
        build_separator_row(widths, theme, 0)
    }
}

/// Box-drawing separator row between header and body. `start_col` is
/// the first column index to draw — the interactive view starts at the
/// horizontal-scroll cursor, print mode always starts at 0.
fn build_separator_row(widths: &[usize], theme: &PeekTheme, start_col: usize) -> String {
    let mut buf = String::new();
    buf.push(SEP_ROW_CHAR);
    for (i, w) in widths.iter().enumerate().skip(start_col) {
        if i > start_col {
            buf.push(SEP_ROW_CHAR);
            buf.push(SEP_JUNCTION_CHAR);
            buf.push(SEP_ROW_CHAR);
        }
        for _ in 0..*w {
            buf.push(SEP_ROW_CHAR);
        }
    }
    theme.paint_muted(&buf)
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::input::InputSource;
    use crate::theme::{PeekThemeName, StyleMode, ThemeManager};
    use crate::types::csv::compose::{build_csv_mode, infer_alignments};
    use crate::types::csv::format::CsvFormat;
    use crate::types::csv::parse::CsvData;
    use bytes::Bytes;

    fn stdin(text: &str) -> InputSource {
        InputSource::stdin(Bytes::copy_from_slice(text.as_bytes()))
    }

    /// Build a `PeekTheme` for the render-function tests.
    fn theme_manager() -> Rc<ThemeManager> {
        Rc::new(ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain))
    }

    fn mode_from(text: &str, fmt: CsvFormat) -> RowsTableMode {
        let src = stdin(text);
        let data = CsvData::open(&src, fmt).unwrap();
        build_csv_mode(data)
    }

    #[test]
    fn seed_widths_grow_with_widest_seed_cell() {
        let mode = mode_from("name,age\nalice,30\nelizabeth,99\n", CsvFormat::Csv);
        // Column 0: max("name"=4, "alice"=5, "elizabeth"=9) = 9
        // Column 1: max("age"=3, "30"=2, "99"=2) = 3
        assert_eq!(mode.widths, vec![9, 3]);
    }

    #[test]
    fn scrolldown_advances_top_record_clamped_to_max() {
        let mut mode = mode_from("h\na\nb\nc\nd\n", CsvFormat::Csv);
        mode.cached_cols = 80;
        mode.cached_rows = 5; // 2 reserved for header+sep → 3 body rows

        assert_eq!(mode.body_total(), 4);
        assert!(mode.scroll(Action::ScrollDown));
        assert_eq!(mode.top_record, 1);
        // Bottom should clamp: max_top = 4 - 3 = 1.
        for _ in 0..10 {
            mode.scroll(Action::ScrollDown);
        }
        assert_eq!(mode.top_record, 1, "clamped at max_top");
    }

    #[test]
    fn shift_h_toggles_header() {
        let mut mode = mode_from("name,age\nalice,30\n", CsvFormat::Csv);
        assert!(mode.has_header);
        assert_eq!(mode.handle(Action::ToggleHeader), Handled::Yes);
        assert!(!mode.has_header);
        assert_eq!(mode.body_total(), 2, "header off → row 1 is body");
    }

    #[test]
    fn shift_r_reflows_widths_to_viewport() {
        // After scrolling past a wide-cell block, Shift+R recomputes from
        // the visible window to reclaim space.
        let mut mode = mode_from(
            "a,b\nshort,x\nmuchlongercell,y\nshort,z\nshort,w\nshort,v\n",
            CsvFormat::Csv,
        );
        mode.cached_cols = 80;
        mode.cached_rows = 4; // 2 reserved → 2 body rows visible
        assert!(
            mode.widths[0] >= "muchlongercell".len(),
            "seed widens for the long cell"
        );

        // Scroll past the long cell so it's no longer in view.
        mode.top_record = 2;
        // Now reflow.
        assert_eq!(mode.handle(Action::ReflowWidths), Handled::Yes);
        // Visible rows are now "short,w" / "short,v" → max width for col 0
        // should be 5 (or header-width "a"=1, whichever larger) plus possibly the header row.
        assert!(
            mode.widths[0] <= "muchlongercell".len(),
            "reflow shouldn't keep the old max"
        );
    }

    #[test]
    fn scroll_right_steps_by_column_clamped_at_last() {
        let mut mode = mode_from("a,b,c\n1,2,3\n", CsvFormat::Csv);
        mode.cached_cols = 80;
        mode.cached_rows = 5;
        assert_eq!(mode.h_col, 0);
        mode.scroll(Action::ScrollRight);
        assert_eq!(mode.h_col, 1);
        mode.scroll(Action::ScrollRight);
        assert_eq!(mode.h_col, 2);
        // Clamp at last column.
        mode.scroll(Action::ScrollRight);
        assert_eq!(mode.h_col, 2);
        mode.scroll(Action::ScrollLeft);
        assert_eq!(mode.h_col, 1);
    }

    #[test]
    fn status_segments_show_record_position_and_column_count() {
        let mode = mode_from("a,b\n1,2\n3,4\n", CsvFormat::Csv);
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        let segs = mode.status_segments(&theme);
        assert!(segs.iter().any(|(s, _)| s == "1/2"));
        assert!(segs.iter().any(|(s, _)| s == "col 1/2"));
    }

    #[test]
    fn numeric_columns_right_align() {
        // `id`, `salary` are numeric; `name`, `department`, `start_date`,
        // `active` are not. Right-align matches the numeric columns only.
        let mode = mode_from("id,name,age\n1,Alice,30\n2,Bob,25\n", CsvFormat::Csv);
        assert_eq!(
            mode.align,
            vec![Alignment::Right, Alignment::Left, Alignment::Right]
        );
    }

    #[test]
    fn render_cell_right_align_puts_pad_on_left() {
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        let out = render_cell(
            "42",
            5,
            theme.foreground,
            Alignment::Right,
            &theme,
            &[],
            None,
        );
        // Three pad spaces precede the content.
        assert!(out.starts_with("   "));
        assert!(out.contains("42"));
    }

    #[test]
    fn body_cell_distinguishes_null_from_empty_and_literal() {
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        let cells = vec![Some("NULL".to_string()), None, Some(String::new())];

        // A literal "NULL" string keeps the base color.
        let (text, color) = body_cell(&cells, 0, theme.foreground, &theme);
        assert_eq!(text, "NULL");
        assert_eq!(color, theme.foreground);

        // A SQL NULL renders the marker, painted muted.
        let (text, color) = body_cell(&cells, 1, theme.foreground, &theme);
        assert_eq!(text, NULL_MARKER);
        assert_eq!(color, theme.muted);
        assert_ne!(theme.muted, theme.foreground, "marker must read distinctly");

        // An empty-string value stays empty (not the NULL marker).
        let (text, color) = body_cell(&cells, 2, theme.foreground, &theme);
        assert_eq!(text, "");
        assert_eq!(color, theme.foreground);

        // A column absent from the row (ragged) is the empty string.
        let (text, _) = body_cell(&cells, 9, theme.foreground, &theme);
        assert_eq!(text, "");
    }

    #[test]
    fn render_cell_left_align_puts_pad_on_right() {
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        let out = render_cell(
            "hi",
            5,
            theme.foreground,
            Alignment::Left,
            &theme,
            &[],
            None,
        );
        assert!(out.ends_with("   "));
    }

    // --- Fixture-based tests ------------------------------------------------

    use std::path::PathBuf;

    fn fixture(rel: &str) -> InputSource {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push(rel);
        InputSource::File(p)
    }

    /// books.csv has two records with embedded `\n` in their description
    /// cell. Rendering must collapse the cell to a single visual row —
    /// no raw newline can survive into the output, or the terminal
    /// breaks alignment for following columns.
    #[test]
    fn fixture_books_multiline_cells_have_no_raw_newlines() {
        let src = fixture("test-data/books.csv");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        // Sanity: fixture still carries embedded newlines.
        let multi = data
            .records
            .iter()
            .filter(|r| !r.malformed)
            .filter(|r| {
                r.cells
                    .iter()
                    .any(|c| c.as_deref().is_some_and(|s| s.contains('\n')))
            })
            .count();
        assert_eq!(multi, 2, "books.csv should have two multi-line cells");

        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        // Render every record; no rendered line may contain a literal `\n`.
        for rec in &data.records {
            if rec.malformed {
                continue;
            }
            for (i, cell) in rec.cells.iter().enumerate() {
                let w = 60usize;
                let align = Alignment::Left;
                let raw = cell.as_deref().unwrap_or("");
                let rendered = render_cell(raw, w, theme.foreground, align, &theme, &[], None);
                assert!(
                    !rendered.contains('\n'),
                    "row {i} cell rendered with embedded newline: {rendered:?}"
                );
                assert!(
                    !rendered.contains('\r'),
                    "row {i} cell rendered with embedded CR: {rendered:?}"
                );
            }
        }
    }

    /// books.csv embedded-newline cells render with the `↵` marker.
    #[test]
    fn fixture_books_embedded_newline_renders_as_marker() {
        let src = fixture("test-data/books.csv");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        let multi = data
            .records
            .iter()
            .find(|r| {
                !r.malformed
                    && r.cells
                        .iter()
                        .any(|c| c.as_deref().is_some_and(|s| s.contains('\n')))
            })
            .expect("at least one multi-line row");
        let cell = multi
            .cells
            .iter()
            .find_map(|c| c.as_deref().filter(|s| s.contains('\n')))
            .unwrap();
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        let rendered = render_cell(
            cell,
            120,
            theme.foreground,
            Alignment::Left,
            &theme,
            &[],
            None,
        );
        assert!(
            rendered.contains('\u{21B5}'),
            "embedded \\n should render as ↵, got: {rendered}"
        );
    }

    /// employees.csv: header detected, 6 columns, numeric columns
    /// (`id`, `salary`) right-aligned.
    #[test]
    fn fixture_employees_alignment_and_header() {
        let src = fixture("test-data/employees.csv");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert_eq!(data.delimiter, b',');
        assert!(data.header_heuristic, "header row detected");
        assert_eq!(data.column_count(), 6);
        let body_start = if data.header_heuristic { 1 } else { 0 };
        let aligns = infer_alignments(&data, body_start);
        // id (int), name (text), department (text), salary (float),
        // start_date (date), active (bool).
        assert_eq!(aligns[0], Alignment::Right, "id column");
        assert_eq!(aligns[1], Alignment::Left, "name column");
        assert_eq!(aligns[3], Alignment::Right, "salary column");
        assert_eq!(aligns[4], Alignment::Left, "start_date column");
    }

    /// measurements.tsv uses tab delimiter via extension.
    #[test]
    fn fixture_measurements_tsv_tab_delimiter() {
        let src = fixture("test-data/measurements.tsv");
        let data = CsvData::open(&src, CsvFormat::Tsv).unwrap();
        assert_eq!(data.delimiter, b'\t');
        assert!(data.header_heuristic);
        assert_eq!(data.column_count(), 6);
    }

    /// euro-prices.csv uses `;` despite the `.csv` extension — the
    /// content sniff overrides the default.
    #[test]
    fn fixture_euro_prices_sniffs_semicolon() {
        let src = fixture("test-data/euro-prices.csv");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert_eq!(data.delimiter, b';', "semicolon should win over comma");
        assert!(data.header_heuristic);
    }

    /// sensor-log.csv has no header — row 0 begins with a numeric Unix
    /// timestamp, so the heuristic must classify it as data.
    #[test]
    fn fixture_sensor_log_no_header() {
        let src = fixture("test-data/sensor-log.csv");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert!(!data.header_heuristic, "row 0 typed → no header");
        assert_eq!(data.column_count(), 5);
    }

    // --- Search -------------------------------------------------------------

    fn make_mode_from_str(text: &str) -> RowsTableMode {
        let mut mode = mode_from(text, CsvFormat::Csv);
        mode.cached_cols = 80;
        mode.cached_rows = 10;
        mode
    }

    #[test]
    fn search_finds_matches_in_cells_only() {
        let mut mode =
            make_mode_from_str("name,city\nAlice,Helsinki\nBob,Helsingborg\nCarol,Tampere\n");
        mode.set_search(Some("Helsi"));
        let s = mode.search.as_ref().expect("search armed");
        assert_eq!(s.matches.len(), 2, "two cells start with Helsi");
        // First match: record_idx 1 (Alice / Helsinki), col_idx 1.
        assert_eq!(s.matches[0].record_idx, 1);
        assert_eq!(s.matches[0].col_idx, 1);
        assert_eq!(s.matches[1].record_idx, 2);
        assert_eq!(s.matches[1].col_idx, 1);
    }

    #[test]
    fn search_does_not_match_across_cells() {
        // Substring "Alice,30" appears only across the field separator.
        let mut mode = make_mode_from_str("name,age\nAlice,30\nBob,25\n");
        mode.set_search(Some("Alice,30"));
        let s = mode.search.as_ref().unwrap();
        assert_eq!(
            s.matches.len(),
            0,
            "match must stay inside one cell — no cross-delimiter join"
        );
    }

    #[test]
    fn search_step_wraps_and_pans_h_col() {
        let mut mode = make_mode_from_str("a,b,c\nfoo,x,y\nbar,foo,z\nbaz,w,foo\n");
        mode.set_search(Some("foo"));
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.matches.len(), 3);
        // Cursor on first match: col 0 → h_col panned to 0.
        assert_eq!(s.cursor, 0);
        assert_eq!(mode.h_col, 0);

        mode.handle(Action::NextMatch);
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.cursor, 1);
        // Second match is in col 1.
        assert_eq!(mode.h_col, 1);

        mode.handle(Action::NextMatch);
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.cursor, 2);
        assert_eq!(mode.h_col, 2);

        // Wrap to first match.
        mode.handle(Action::NextMatch);
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.cursor, 0);

        // Backward wraps the other way.
        mode.handle(Action::PrevMatch);
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.cursor, 2);
    }

    #[test]
    fn search_smart_case() {
        // All-lowercase query is case-insensitive.
        let mut mode = make_mode_from_str("city\nHelsinki\nhelsinki\nOulu\n");
        mode.set_search(Some("helsinki"));
        assert_eq!(mode.search.as_ref().unwrap().matches.len(), 2);
        // Mixed case query is case-sensitive.
        mode.set_search(Some("Helsinki"));
        assert_eq!(mode.search.as_ref().unwrap().matches.len(), 1);
    }

    #[test]
    fn search_empty_query_clears() {
        let mut mode = make_mode_from_str("a\nfoo\n");
        mode.set_search(Some("foo"));
        assert!(mode.search.is_some());
        mode.set_search(None);
        assert!(mode.search.is_none());
        mode.set_search(Some("foo"));
        assert!(mode.search.is_some());
        mode.set_search(Some(""));
        assert!(mode.search.is_none());
    }

    #[test]
    fn back_clears_search() {
        let mut mode = make_mode_from_str("a\nfoo\n");
        mode.set_search(Some("foo"));
        assert_eq!(mode.handle(Action::Back), Handled::Yes);
        assert!(mode.search.is_none());
        // Second Back with no search falls through.
        assert_eq!(mode.handle(Action::Back), Handled::No);
    }

    #[test]
    fn search_status_segment_shows_position() {
        // "no match" is unique to the search segment, so it's the cleaner
        // signal — the row-position segment can collide with `M/N` shapes.
        let mut mode = make_mode_from_str("a\nfoo\nfoo\n");
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        mode.set_search(Some("zzz"));
        let segs = mode.status_segments(&theme);
        assert!(segs.iter().any(|(s, _)| s == "no match"));
        mode.set_search(None);
        let segs = mode.status_segments(&theme);
        assert!(!segs.iter().any(|(s, _)| s == "no match"));
    }

    #[test]
    fn search_matches_inside_multiline_cell_use_display_form() {
        // books.csv has a record whose description spans two lines via
        // an embedded \n. Searching the display-form text — across the
        // `↵` glyph — must still locate the match.
        let src = fixture("test-data/books.csv");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        let mut mode = build_csv_mode(data);
        mode.cached_cols = 200;
        mode.cached_rows = 30;
        // "Includes worked examples" lives on the second physical line
        // of the Refactoring book's description cell.
        mode.set_search(Some("Includes worked"));
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.matches.len(), 1);
        // It's in column 3 (description), not the title / author columns.
        assert_eq!(s.matches[0].col_idx, 3);
    }
}
