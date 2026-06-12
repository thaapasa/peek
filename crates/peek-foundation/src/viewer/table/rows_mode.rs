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
    MAX_MATCHES, SEARCH_SCAN_MAX_BYTES, ScanStop, SearchTarget, count_status_label, find_matches,
    overlay_matches, smart_case_sensitive, truncated_scan_warning,
};
use crate::viewer::table::row_source::RowSource;
use crate::viewer::ui::{Action, HelpEntry, take_cols, truncate_ansi};

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
pub enum Alignment {
    Left,
    Right,
}

pub struct RowsTableMode {
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
    /// Warnings produced by recent operations (a truncated search scan),
    /// drained by `take_warnings` into `FileInfo.warnings`.
    pending_warnings: Vec<String>,
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
    /// `Some` when the scan stopped early, with the cause. Matches past
    /// the stop point are unknown; the status segment marks the counts
    /// as partial.
    stop: Option<ScanStop>,
}

#[derive(Clone)]
struct CellMatch {
    record_idx: usize,
    col_idx: usize,
    /// Byte range inside the cell's display form.
    range: Range<usize>,
}

/// Search-match overlay spec for a single cell passed to [`render_cell`]:
/// byte ranges into the cell's display form plus which of them is the
/// active cursor match. An empty `ranges` is the no-search case.
struct CellMatches<'a> {
    ranges: &'a [Range<usize>],
    current: Option<usize>,
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
    pub fn new(
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
            pending_warnings: Vec::new(),
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

    /// Build a [`CellSearch`] over up to `max_bytes` of record bytes.
    /// Each record is pulled into the source's sliding window via
    /// `ensure_row` just before it's read, so the scan walks the file
    /// without ever holding more than one window in memory. The walk is
    /// budgeted like every streaming scan ([`SEARCH_SCAN_MAX_BYTES`] /
    /// [`MAX_MATCHES`]) — an unindexed multi-GB CSV would otherwise
    /// freeze the UI for a full file read on every zero-hit query.
    /// Every record is charged [`RowSource::row_scan_bytes`], malformed
    /// ones included — they're parsed (and paid for) before the
    /// malformed flag is known, so a budget that skipped them would walk
    /// a mostly-malformed multi-GB file end-to-end. Stopping early marks
    /// the search truncated so the status segment reports the counts as
    /// partial.
    fn build_search_capped(&mut self, query: &str, max_bytes: u64) -> CellSearch {
        let sensitive = smart_case_sensitive(query);
        let mut matches: Vec<CellMatch> = Vec::new();
        let cols = self.widths.len();
        let mut stop = None;
        let mut scanned: u64 = 0;
        let mut record_idx = 0usize;
        'records: loop {
            // An I/O error ends the scan early — the walk didn't cover
            // the file, so the counts must report as partial, same as a
            // budget stop.
            let Ok(bound) = self.source.ensure_row(record_idx) else {
                stop = Some(ScanStop::Error);
                break;
            };
            if record_idx >= bound {
                break;
            }
            if scanned >= max_bytes {
                stop = Some(ScanStop::ByteBudget);
                break;
            }
            scanned += self.source.row_scan_bytes(record_idx);
            if !self.source.row_is_malformed(record_idx)
                && let Some(cells) = self.source.row(record_idx)
            {
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
                            stop = Some(ScanStop::MatchCap);
                            break 'records;
                        }
                    }
                }
            }
            record_idx += 1;
        }
        CellSearch {
            matches,
            cursor: 0,
            stop,
        }
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
    /// emitting it). `start_col` is the first column to draw and
    /// `overflow` chooses spill-vs-truncate — the interactive view passes
    /// `(self.h_col, false)`, the print path `(0, true)`.
    fn build_header_row(
        &self,
        widths: &[usize],
        theme: &PeekTheme,
        start_col: usize,
        overflow: bool,
    ) -> String {
        if !self.has_header {
            return String::new();
        }
        let Some(cells) = self.source.row(0) else {
            return String::new();
        };
        let mut out = String::new();
        out.push(' ');
        for (i, w) in widths.iter().enumerate().skip(start_col) {
            if i > start_col {
                out.push_str(&theme.paint_muted(COL_SEP));
            }
            let cell = cells.get(i).and_then(|c| c.as_deref()).unwrap_or("");
            let align = self.align.get(i).copied().unwrap_or(Alignment::Left);
            let (ranges, current) = self.cell_match_ranges(0, i);
            let matches = CellMatches {
                ranges: &ranges,
                current,
            };
            let painted = render_cell(cell, *w, theme.heading, align, theme, matches, overflow);
            out.push_str(&painted);
        }
        out
    }

    /// Build the separator row between header and body.
    fn build_separator_row(&self, widths: &[usize], theme: &PeekTheme) -> String {
        build_separator_row(widths, theme, self.h_col)
    }

    /// Build one body row at record index `body_idx` (0 = first body row).
    /// A malformed record paints every column `<error>` with
    /// `theme.warning`. `start_col` / `overflow` follow the same
    /// interactive-vs-print convention as [`build_header_row`].
    fn build_body_row(
        &self,
        body_idx: usize,
        widths: &[usize],
        theme: &PeekTheme,
        start_col: usize,
        overflow: bool,
    ) -> Option<String> {
        let rec_idx = self.body_start() + body_idx;
        let malformed = self.source.row_is_malformed(rec_idx);
        let cells = self.source.row(rec_idx)?;
        let mut out = String::new();
        out.push(' ');
        for (i, w) in widths.iter().enumerate().skip(start_col) {
            if i > start_col {
                out.push_str(&theme.paint_muted(COL_SEP));
            }
            let (cell, color): (&str, Color) = if malformed {
                ("<error>", theme.warning)
            } else {
                body_cell(cells, i, theme.foreground, theme)
            };
            let align = self.align.get(i).copied().unwrap_or(Alignment::Left);
            let (ranges, current) = self.cell_match_ranges(rec_idx, i);
            let matches = CellMatches {
                ranges: &ranges,
                current,
            };
            out.push_str(&render_cell(
                cell, *w, color, align, theme, matches, overflow,
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
/// `matches` carries byte offsets into the cell's *display* form, used
/// to overlay search-match backgrounds, plus which range is the active
/// cursor match (the brighter background). Ranges that fall in the
/// truncated tail are dropped. Pass [`CellMatches { ranges: &[], current: None }`] when no
/// search is active.
///
/// When `overflow` is set, a cell wider than `width` is emitted in full
/// (no truncation marker, no padding) and spills past the column edge —
/// the print path uses this so a wide cell prints whole; the next row
/// realigns. The interactive path passes `false` to keep the grid tight.
fn render_cell(
    cell: &str,
    width: usize,
    color: Color,
    align: Alignment,
    theme: &PeekTheme,
    matches: CellMatches,
    overflow: bool,
) -> String {
    let display = display_cell(cell);
    let cell_w = display.width();
    let (content, prefix_len, pad): (Cow<str>, usize, usize) = if cell_w > width && !overflow {
        let t = take_cols(&display, width.saturating_sub(1));
        let prefix = t.len();
        let mut c = t;
        c.push(TRUNCATE_MARKER);
        (Cow::Owned(c), prefix, 0)
    } else if cell_w > width {
        let len = display.len();
        (display, len, 0)
    } else {
        let len = display.len();
        (display, len, width - cell_w)
    };

    let mut inner = String::with_capacity(content.len() + 24);
    paint_content_with_markers(&mut inner, &content, color, theme);

    if !matches.ranges.is_empty() {
        let (kept, kept_current) =
            filter_ranges_for_prefix(matches.ranges, matches.current, prefix_len);
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
pub fn display_cell(s: &str) -> Cow<'_, str> {
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

        // Clip each row to the viewport width. Rows span every column
        // from `h_col` to the last, which can sum past the terminal; the
        // ScreenBuffer writes lines verbatim, so an over-wide line would
        // soft-wrap onto the row below. Horizontal panning (`h_col`)
        // brings off-screen-right columns into view — the clip is the
        // right edge of that window.
        let clip = |line: String| truncate_ansi(&line, ctx.term_cols);

        if self.has_header {
            let header_row = self.build_header_row(&widths, ctx.peek_theme, self.h_col, false);
            if !header_row.is_empty() {
                lines.push(clip(header_row));
            }
            lines.push(clip(self.build_separator_row(&widths, ctx.peek_theme)));
        }

        let total_body = self.body_total();
        let mut emitted = 0;
        let mut body_idx = self.top_record;
        while emitted < body_rows && body_idx < total_body {
            if let Some(row) =
                self.build_body_row(body_idx, &widths, ctx.peek_theme, self.h_col, false)
            {
                lines.push(clip(row));
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
            // Print mode draws every column from index 0 and lets wide
            // cells spill past the column edge (`overflow = true`).
            out.write_line(&self.build_header_row(&widths, ctx.peek_theme, 0, true))?;
            out.write_line(&build_separator_row(&widths, ctx.peek_theme, 0))?;
        }
        let body_start = self.body_start();
        let loaded = self.source.loaded();
        for rec_idx in body_start..loaded {
            if self.source.row_is_malformed(rec_idx) {
                // Whole-row marker — print mode collapses a malformed
                // record to one spanning line rather than per-column.
                let mut row = String::from(' ');
                row.push_str(
                    &ctx.peek_theme
                        .paint("<malformed record>", ctx.peek_theme.warning),
                );
                out.write_line(&row)?;
                continue;
            }
            if let Some(row) =
                self.build_body_row(rec_idx - body_start, &widths, ctx.peek_theme, 0, true)
            {
                out.write_line(&row)?;
            }
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
            Action::Next => {
                self.step_match(1);
                Handled::Yes
            }
            Action::Prev => {
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
        // Search position, shown only while a search is active. A
        // truncated scan marks the counts as lower bounds (`3/40+`,
        // `no match (partial scan)`) so they never claim full-file
        // coverage they don't have.
        if let Some(s) = &self.search {
            segs.push((
                count_status_label(s.cursor, s.matches.len(), s.stop.is_some()),
                theme.label,
            ));
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
        let search = self.build_search_capped(query, SEARCH_SCAN_MAX_BYTES);
        if let Some(stop) = search.stop {
            // Identical text per push — the session layer dedupes, so
            // repeated truncated queries warn once.
            self.pending_warnings
                .push(truncated_scan_warning(stop, " of table data"));
        }
        self.search = Some(search);
        self.scroll_to_current_match();
        SearchTarget::Owned
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
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
    use crate::info::{FileInfo, NoExtras, RenderOptions};
    use crate::theme::{PeekThemeName, StyleMode, ThemeManager};
    use crate::viewer::ui::strip_ansi_width;

    /// In-memory [`RowSource`] for the table-mode mechanics tests. The CSV
    /// reader's own behaviour (quoting, delimiter sniff, header heuristic,
    /// alignment inference) is covered in peek-types; here only
    /// `RowsTableMode`'s scroll / width / search logic is under test, so a
    /// fake source keeps these tests in the foundation layer.
    struct FakeRows {
        rows: Vec<Vec<Option<String>>>,
        cols: usize,
    }

    impl RowSource for FakeRows {
        fn ensure_row(&mut self, _idx: usize) -> Result<usize> {
            Ok(self.rows.len())
        }
        fn ensure_all(&mut self) -> Result<()> {
            Ok(())
        }
        fn row(&self, idx: usize) -> Option<&[Option<String>]> {
            self.rows.get(idx).map(Vec::as_slice)
        }
        fn loaded(&self) -> usize {
            self.rows.len()
        }
        fn total(&self) -> Option<usize> {
            Some(self.rows.len())
        }
        fn column_count(&self) -> usize {
            self.cols
        }
    }

    fn mode_from_cells(rows: Vec<Vec<&str>>) -> RowsTableMode {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        let rows: Vec<Vec<Option<String>>> = rows
            .into_iter()
            .map(|r| r.into_iter().map(|c| Some(c.to_string())).collect())
            .collect();
        let align = vec![Alignment::Left; cols];
        RowsTableMode::new(Box::new(FakeRows { rows, cols }), align, true, "Table")
    }

    /// Build a header-on, all-left-aligned `RowsTableMode` from simple
    /// comma-separated lines. The split is deliberately naive (`,` / `\n`,
    /// no quoting) — fixtures needing real CSV parsing live in peek-types.
    fn mode_from(text: &str) -> RowsTableMode {
        let rows = text
            .lines()
            .map(|line| line.split(',').collect::<Vec<_>>())
            .collect();
        mode_from_cells(rows)
    }

    /// Build a `PeekTheme` for the render-function tests.
    fn theme_manager() -> Rc<ThemeManager> {
        Rc::new(ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain))
    }

    /// Minimal `FileInfo` for a `RenderCtx` without the gather hub — the
    /// render-width tests never read its fields.
    fn synthetic_file_info() -> FileInfo {
        FileInfo {
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
        }
    }

    #[test]
    fn seed_widths_grow_with_widest_seed_cell() {
        let mode = mode_from("name,age\nalice,30\nelizabeth,99\n");
        // Column 0: max("name"=4, "alice"=5, "elizabeth"=9) = 9
        // Column 1: max("age"=3, "30"=2, "99"=2) = 3
        assert_eq!(mode.widths, vec![9, 3]);
    }

    #[test]
    fn scrolldown_advances_top_record_clamped_to_max() {
        let mut mode = mode_from("h\na\nb\nc\nd\n");
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
        let mut mode = mode_from("name,age\nalice,30\n");
        assert!(mode.has_header);
        assert_eq!(mode.handle(Action::ToggleHeader), Handled::Yes);
        assert!(!mode.has_header);
        assert_eq!(mode.body_total(), 2, "header off → row 1 is body");
    }

    #[test]
    fn shift_r_reflows_widths_to_viewport() {
        // After scrolling past a wide-cell block, Shift+R recomputes from
        // the visible window to reclaim space.
        let mut mode = mode_from("a,b\nshort,x\nmuchlongercell,y\nshort,z\nshort,w\nshort,v\n");
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
        let mut mode = mode_from("a,b,c\n1,2,3\n");
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
        let mode = mode_from("a,b\n1,2\n3,4\n");
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        let segs = mode.status_segments(&theme);
        assert!(segs.iter().any(|(s, _)| s == "1/2"));
        assert!(segs.iter().any(|(s, _)| s == "col 1/2"));
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
            CellMatches {
                ranges: &[],
                current: None,
            },
            false,
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
            CellMatches {
                ranges: &[],
                current: None,
            },
            false,
        );
        assert!(out.ends_with("   "));
    }

    // --- Cell rendering of embedded newlines --------------------------------
    //
    // A cell may carry an embedded `\n` (CSV quoted field, SQLite text).
    // The CSV reader's parsing of such fields is covered in peek-types;
    // here only `render_cell`'s collapse-to-one-row behaviour matters, so
    // the inputs are literal multi-line strings.

    /// A multi-line cell must collapse to a single visual row — no raw
    /// `\n` / `\r` can survive into the output, or the terminal breaks
    /// alignment for the following columns.
    #[test]
    fn render_cell_collapses_embedded_newlines() {
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        let rendered = render_cell(
            "first line\nsecond line\r\nthird",
            60,
            theme.foreground,
            Alignment::Left,
            &theme,
            CellMatches {
                ranges: &[],
                current: None,
            },
            false,
        );
        assert!(
            !rendered.contains('\n'),
            "cell rendered with embedded newline: {rendered:?}"
        );
        assert!(
            !rendered.contains('\r'),
            "cell rendered with embedded CR: {rendered:?}"
        );
    }

    /// An embedded `\n` renders as the `↵` marker.
    #[test]
    fn render_cell_embedded_newline_renders_as_marker() {
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        let rendered = render_cell(
            "first line\nsecond line",
            120,
            theme.foreground,
            Alignment::Left,
            &theme,
            CellMatches {
                ranges: &[],
                current: None,
            },
            false,
        );
        assert!(
            rendered.contains('\u{21B5}'),
            "embedded \\n should render as ↵, got: {rendered}"
        );
    }

    // --- Search -------------------------------------------------------------

    fn make_mode_from_str(text: &str) -> RowsTableMode {
        let mut mode = mode_from(text);
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

        mode.handle(Action::Next);
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.cursor, 1);
        // Second match is in col 1.
        assert_eq!(mode.h_col, 1);

        mode.handle(Action::Next);
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.cursor, 2);
        assert_eq!(mode.h_col, 2);

        // Wrap to first match.
        mode.handle(Action::Next);
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.cursor, 0);

        // Backward wraps the other way.
        mode.handle(Action::Prev);
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
        // A description cell spans two lines via an embedded \n. Searching
        // the display-form text — across the `↵` glyph — must still locate
        // a match that straddles the original line break only if the query
        // sits within one physical line; here the query is wholly on the
        // second line, so it matches.
        let mut mode = mode_from_cells(vec![
            vec!["title", "author", "year", "description"],
            vec![
                "Refactoring",
                "Fowler",
                "1999",
                "A catalog of refactorings.\nIncludes worked examples.",
            ],
        ]);
        mode.cached_cols = 200;
        mode.cached_rows = 30;
        mode.set_search(Some("Includes worked"));
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.matches.len(), 1);
        // It's in column 3 (description), not the title / author columns.
        assert_eq!(s.matches[0].col_idx, 3);
    }

    /// Lazily-loading [`RowSource`] for the search-walk tests: rows
    /// materialise only as `ensure_row` asks for them, like the CSV
    /// sliding window. Pins that the search walk drives the source
    /// forward itself instead of relying on a prior `ensure_all`.
    struct LazyRows {
        rows: Vec<Vec<Option<String>>>,
        loaded: usize,
    }

    impl RowSource for LazyRows {
        fn ensure_row(&mut self, idx: usize) -> Result<usize> {
            self.loaded = self.loaded.max((idx + 1).min(self.rows.len()));
            Ok(self.loaded)
        }
        fn ensure_all(&mut self) -> Result<()> {
            self.loaded = self.rows.len();
            Ok(())
        }
        fn row(&self, idx: usize) -> Option<&[Option<String>]> {
            (idx < self.loaded).then(|| self.rows[idx].as_slice())
        }
        fn loaded(&self) -> usize {
            self.loaded
        }
        fn total(&self) -> Option<usize> {
            None
        }
        fn column_count(&self) -> usize {
            1
        }
    }

    fn lazy_mode(cells: &[&str]) -> RowsTableMode {
        let rows = cells.iter().map(|c| vec![Some(c.to_string())]).collect();
        RowsTableMode::new(
            Box::new(LazyRows { rows, loaded: 1 }),
            vec![Alignment::Left],
            false,
            "Table",
        )
    }

    /// The search walk must pull records past the initially-loaded seed
    /// all the way to EOF (when within budget) — a match in the last,
    /// not-yet-materialised record is still found.
    #[test]
    fn search_walks_lazy_source_to_end_within_budget() {
        let mut mode = lazy_mode(&["alpha", "beta", "needle"]);
        assert_eq!(mode.source.loaded(), 1, "only the seed row loaded");
        mode.set_search(Some("needle"));
        let s = mode.search.as_ref().unwrap();
        assert_eq!(s.matches.len(), 1);
        assert_eq!(s.matches[0].record_idx, 2);
        assert!(s.stop.is_none());
    }

    /// The record walk stops at the byte budget instead of reading the
    /// whole source — the M17 freeze for tables. Matches past the stop
    /// point are unknown, so the result is marked truncated.
    #[test]
    fn search_record_walk_stops_at_byte_budget() {
        let cells: Vec<String> = (0..100).map(|i| format!("hit {i:04}")).collect();
        let refs: Vec<&str> = cells.iter().map(String::as_str).collect();
        let mut mode = lazy_mode(&refs);

        // 8-byte cells; a 30-byte budget admits only a few records.
        let s = mode.build_search_capped("hit", 30);
        assert_eq!(s.stop, Some(ScanStop::ByteBudget));
        let n = s.matches.len();
        assert!(
            (1..100).contains(&n),
            "should find some but not all matches, got {n}"
        );

        // Unbounded budget: complete and not truncated.
        let s = mode.build_search_capped("hit", u64::MAX);
        assert!(s.stop.is_none());
        assert_eq!(s.matches.len(), 100);
    }

    /// Lazy source whose rows are all malformed: zero cell text, but a
    /// real per-record parse cost reported via `row_scan_bytes` — the
    /// shape of a CSV gone malformed after a stray quote. `max_ensured`
    /// is shared out so the test can observe how deep the walk went.
    struct MalformedRows {
        total: usize,
        row_cost: u64,
        max_ensured: Rc<std::cell::Cell<usize>>,
    }

    impl RowSource for MalformedRows {
        fn ensure_row(&mut self, idx: usize) -> Result<usize> {
            self.max_ensured.set(self.max_ensured.get().max(idx));
            Ok(self.total)
        }
        fn ensure_all(&mut self) -> Result<()> {
            Ok(())
        }
        fn row(&self, _idx: usize) -> Option<&[Option<String>]> {
            Some(&[])
        }
        fn row_is_malformed(&self, _idx: usize) -> bool {
            true
        }
        fn row_scan_bytes(&self, _idx: usize) -> u64 {
            self.row_cost
        }
        fn loaded(&self) -> usize {
            self.total
        }
        fn total(&self) -> Option<usize> {
            Some(self.total)
        }
        fn column_count(&self) -> usize {
            1
        }
        fn malformed_count(&self) -> usize {
            self.total
        }
    }

    /// Malformed records expose no cell text but still cost a parse, so
    /// they must be charged to the byte budget — otherwise a
    /// mostly-malformed multi-GB file is re-walked end-to-end on every
    /// query, the exact freeze the budget exists to prevent.
    #[test]
    fn search_budget_charges_malformed_records() {
        let max_ensured = Rc::new(std::cell::Cell::new(0));
        let mut mode = RowsTableMode::new(
            Box::new(MalformedRows {
                total: 1000,
                row_cost: 10,
                max_ensured: Rc::clone(&max_ensured),
            }),
            vec![Alignment::Left],
            false,
            "Table",
        );
        // 1000 records × 10 bytes each; a 100-byte budget admits ~10.
        let s = mode.build_search_capped("x", 100);
        assert_eq!(s.stop, Some(ScanStop::ByteBudget));
        assert!(s.matches.is_empty(), "malformed rows produce no matches");
        let walked = max_ensured.get();
        assert!(
            walked <= 12,
            "walk must stop near the budget, not cover all 1000 records; ensured up to {walked}"
        );
    }

    /// [`RowSource`] whose pull fails partway through, like an I/O error
    /// mid-file. Wraps [`LazyRows`] and errors past `fail_at`.
    struct FailingRows {
        inner: LazyRows,
        fail_at: usize,
    }

    impl RowSource for FailingRows {
        fn ensure_row(&mut self, idx: usize) -> Result<usize> {
            if idx >= self.fail_at {
                anyhow::bail!("synthetic read error");
            }
            self.inner.ensure_row(idx)
        }
        fn ensure_all(&mut self) -> Result<()> {
            anyhow::bail!("synthetic read error");
        }
        fn row(&self, idx: usize) -> Option<&[Option<String>]> {
            self.inner.row(idx)
        }
        fn loaded(&self) -> usize {
            self.inner.loaded()
        }
        fn total(&self) -> Option<usize> {
            None
        }
        fn column_count(&self) -> usize {
            1
        }
    }

    /// An I/O error mid-walk ends the scan without covering the file, so
    /// the result must be marked truncated — otherwise the status bar
    /// reports an error-terminated count as complete coverage.
    #[test]
    fn search_record_walk_error_marks_truncated() {
        let rows = (0..10).map(|i| vec![Some(format!("hit {i}"))]).collect();
        let mut mode = RowsTableMode::new(
            Box::new(FailingRows {
                inner: LazyRows { rows, loaded: 1 },
                fail_at: 3,
            }),
            vec![Alignment::Left],
            false,
            "Table",
        );
        let s = mode.build_search_capped("hit", u64::MAX);
        assert_eq!(
            s.stop,
            Some(ScanStop::Error),
            "error-terminated scan must report partial"
        );
        assert_eq!(s.matches.len(), 3, "matches up to the failure point");
    }

    /// Truncated counts must read as lower bounds in the status bar.
    #[test]
    fn search_status_marks_truncated_counts_partial() {
        let cells: Vec<String> = (0..50).map(|i| format!("hit {i:04}")).collect();
        let refs: Vec<&str> = cells.iter().map(String::as_str).collect();
        let mut mode = lazy_mode(&refs);
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();

        let s = mode.build_search_capped("hit", 30);
        assert!(s.stop.is_some());
        mode.search = Some(s);
        let segs = mode.status_segments(&theme);
        assert!(
            segs.iter()
                .any(|(t, _)| t.starts_with("1/") && t.ends_with('+')),
            "truncated count must end with '+': {segs:?}"
        );

        let s = mode.build_search_capped("zzz", 30);
        assert!(s.stop.is_some());
        mode.search = Some(s);
        let segs = mode.status_segments(&theme);
        assert!(
            segs.iter().any(|(t, _)| t == "no match (partial scan)"),
            "zero-hit truncated scan must say partial: {segs:?}"
        );
    }

    /// A table wider than the viewport must not emit a line wider than
    /// the terminal — the ScreenBuffer writes lines verbatim, so an
    /// over-wide line would soft-wrap onto the row below. Holds at the
    /// left edge and after panning right.
    #[test]
    fn rendered_rows_never_exceed_viewport_width() {
        // Four wide columns: the full row is far wider than 24 cols.
        let text = "alpha,bravo,charlie,delta\n\
                    wide_value_one,wide_value_two,wide_value_three,wide_value_four\n\
                    another_long_a,another_long_b,another_long_c,another_long_d\n";
        let file_info = synthetic_file_info();
        let mut mode = mode_from(text);

        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        let cols = 24;
        let ctx = RenderCtx {
            file_info: &file_info,
            theme_name: PeekThemeName::IdeaDark,
            peek_theme: &theme,
            render_opts: RenderOptions::default(),
            term_cols: cols,
            term_rows: 10,
        };

        let assert_within = |mode: &mut RowsTableMode| {
            let win = mode.render_window(&ctx, 0, 10).unwrap();
            for line in &win.lines {
                assert!(
                    strip_ansi_width(line) <= cols,
                    "line wider than {cols} cols ({}): {line:?}",
                    strip_ansi_width(line),
                );
            }
        };

        assert_within(&mut mode);
        // Pan right twice — later columns become the left edge; the row
        // from there to the last column still must not overflow.
        mode.scroll(Action::ScrollRight);
        mode.scroll(Action::ScrollRight);
        assert_within(&mut mode);
    }
}
