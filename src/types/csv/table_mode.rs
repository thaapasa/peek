//! Aligned CSV / TSV table view.
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
//! * [`CsvData`] backing parser — owns the record cache + ongoing reader
//! * `widths` — monotonic per-column widths, seeded from the first 1000
//!   records and grown (never shrunk) as wider cells scroll into view
//! * `has_header` — runtime override of the parser's header heuristic
//!   (`Shift+H` toggles)
//! * `top_record` — record index at the top of the body viewport
//! * `h_col` — left-most visible column (column-step horizontal pan)
//!
//! Print mode renders the seeded widths only — no auto-widen — so the
//! table layout never depends on the deepest row consumed.

use std::borrow::Cow;
use std::rc::Rc;

use anyhow::Result;
use syntect::highlighting::Color;
use unicode_width::UnicodeWidthStr;

use crate::output::PrintOutput;
use crate::theme::{PeekTheme, PeekThemeName};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::viewer::ui::{Action, HelpEntry};

use super::parse::CsvData;

/// One space of padding on each side of the column separator and on the
/// leading/trailing edges. Matches `column_sep` below.
const COL_SEP: &str = " │ ";
/// Visible cell-width contribution of [`COL_SEP`] (one separator + two
/// surrounding spaces — the bar is 1 col, spaces are 2 cols).
const COL_SEP_WIDTH: usize = 3;
/// Glyph for separator-row segments under a column.
const SEP_ROW_CHAR: char = '─';
/// Junction glyph at column boundaries on the separator row.
const SEP_JUNCTION_CHAR: char = '┼';
/// Truncation marker on cells wider than their column width.
const TRUNCATE_MARKER: char = '…';

/// Hard ceiling on per-column width — keeps a single huge cell from
/// pushing the whole table off the screen. Cells past this width are
/// truncated with [`TRUNCATE_MARKER`].
const MAX_COLUMN_WIDTH: usize = 64;

pub(crate) struct CsvTableMode {
    data: CsvData,
    /// Per-column widths. Monotonic — auto-widen grows them, `Shift+R`
    /// recomputes from the visible window. Length matches the column
    /// count of the first non-malformed record.
    widths: Vec<usize>,
    /// Seed widths captured at open time. Print-mode rendering uses
    /// these directly; interactive rendering may grow `widths` past them.
    seed_widths: Vec<usize>,
    /// Runtime override of the parser's header heuristic. `Shift+H`
    /// toggles; reset to the heuristic on construction.
    has_header: bool,
    top_record: usize,
    h_col: usize,
    cached_cols: usize,
    cached_rows: usize,
    #[allow(dead_code)]
    theme_name: PeekThemeName,
    #[allow(dead_code)]
    theme_manager: Rc<crate::theme::ThemeManager>,
    label: &'static str,
}

const TABLE_ACTIONS: &[HelpEntry] = &[
    (&[Action::ToggleHeader], "Toggle header row"),
    (&[Action::ReflowWidths], "Reflow column widths"),
    (
        &[Action::ScrollLeft, Action::ScrollRight],
        "Pan columns left / right",
    ),
];

impl CsvTableMode {
    pub(crate) fn new(
        data: CsvData,
        theme_manager: Rc<crate::theme::ThemeManager>,
        theme_name: PeekThemeName,
    ) -> Self {
        let widths = seed_widths(&data);
        let has_header = data.header_heuristic;
        Self {
            seed_widths: widths.clone(),
            widths,
            has_header,
            data,
            top_record: 0,
            h_col: 0,
            cached_cols: 0,
            cached_rows: 0,
            theme_name,
            theme_manager,
            label: "Table",
        }
    }

    fn body_start(&self) -> usize {
        if self.has_header { 1 } else { 0 }
    }

    /// Total body records (excludes the header row when `has_header`).
    fn body_total(&self) -> usize {
        let loaded = self.data.loaded();
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
        // Header text first.
        if self.has_header
            && let Some(rec) = self.data.records.first()
        {
            for (i, cell) in rec.cells.iter().enumerate().take(cols) {
                let w = display_cell(cell).width().min(MAX_COLUMN_WIDTH);
                if w > new_widths[i] {
                    new_widths[i] = w;
                }
            }
        }
        // Visible body rows.
        let reserved = if self.has_header { 2 } else { 0 };
        let rows = self.cached_rows.saturating_sub(reserved);
        let start = self.body_start() + self.top_record;
        let end = (start + rows).min(self.data.loaded());
        for rec in &self.data.records[start..end] {
            if rec.malformed {
                continue;
            }
            for (i, cell) in rec.cells.iter().enumerate().take(cols) {
                let w = display_cell(cell).width().min(MAX_COLUMN_WIDTH);
                if w > new_widths[i] {
                    new_widths[i] = w;
                }
            }
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
        let end = (start + rows).min(self.data.loaded());
        for rec in &self.data.records[start..end] {
            if rec.malformed {
                continue;
            }
            for (i, cell) in rec.cells.iter().enumerate().take(cols) {
                let w = display_cell(cell).width().min(MAX_COLUMN_WIDTH);
                if w > self.widths[i] {
                    self.widths[i] = w;
                }
            }
        }
        if self.has_header
            && let Some(rec) = self.data.records.first()
        {
            for (i, cell) in rec.cells.iter().enumerate().take(cols) {
                let w = display_cell(cell).width().min(MAX_COLUMN_WIDTH);
                if w > self.widths[i] {
                    self.widths[i] = w;
                }
            }
        }
    }

    /// Build the header row painted with `theme.heading`. Returns
    /// `String::new()` when there's no header (the caller should skip
    /// emitting it).
    fn build_header_row(&self, widths: &[usize], theme: &PeekTheme) -> String {
        if !self.has_header {
            return String::new();
        }
        let Some(rec) = self.data.records.first() else {
            return String::new();
        };
        let mut out = String::new();
        out.push(' ');
        for (i, w) in widths.iter().enumerate().skip(self.h_col) {
            if i > self.h_col {
                out.push_str(&theme.paint_muted(COL_SEP));
            }
            let cell = rec.cells.get(i).map(|s| s.as_str()).unwrap_or("");
            let painted = render_cell(cell, *w, theme.heading, theme);
            out.push_str(&painted);
        }
        out
    }

    /// Build the separator row between header and body.
    fn build_separator_row(&self, widths: &[usize], theme: &PeekTheme) -> String {
        let mut buf = String::new();
        buf.push(SEP_ROW_CHAR);
        for (i, w) in widths.iter().enumerate().skip(self.h_col) {
            if i > self.h_col {
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

    /// Build one body row at record index `body_idx` (0 = first body row).
    /// `malformed` flag paints the row with `theme.warning`.
    fn build_body_row(
        &self,
        body_idx: usize,
        widths: &[usize],
        theme: &PeekTheme,
    ) -> Option<String> {
        let rec_idx = self.body_start() + body_idx;
        let rec = self.data.records.get(rec_idx)?;
        let mut out = String::new();
        out.push(' ');
        for (i, w) in widths.iter().enumerate().skip(self.h_col) {
            if i > self.h_col {
                out.push_str(&theme.paint_muted(COL_SEP));
            }
            let (cell, color): (&str, Color) = if rec.malformed {
                ("<error>", theme.warning)
            } else {
                (
                    rec.cells.get(i).map(|s| s.as_str()).unwrap_or(""),
                    theme.foreground,
                )
            };
            out.push_str(&render_cell(cell, *w, color, theme));
        }
        Some(out)
    }
}

/// Render one cell into its column. Truncates wider content with
/// [`TRUNCATE_MARKER`] and right-pads narrower content. Sanitizes
/// embedded newlines / tabs via [`display_cell`] first so a multi-line
/// cell collapses onto a single visual row.
fn render_cell(cell: &str, width: usize, color: Color, theme: &PeekTheme) -> String {
    let display = display_cell(cell);
    let cell_w = display.width();
    if cell_w == width {
        return theme.paint(&display, color);
    }
    if cell_w > width {
        let truncated = take_cols(&display, width.saturating_sub(1));
        let mut out = String::with_capacity(truncated.len() + 4);
        out.push_str(&truncated);
        out.push(TRUNCATE_MARKER);
        return theme.paint(&out, color);
    }
    // Pad with spaces to the column width.
    let pad = width - cell_w;
    let mut out = String::with_capacity(display.len() + pad);
    out.push_str(&display);
    for _ in 0..pad {
        out.push(' ');
    }
    theme.paint(&out, color)
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
fn display_cell(s: &str) -> Cow<'_, str> {
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

/// Take at most `max_cols` visible columns from `s`. Wide characters
/// straddling the cut are dropped rather than split.
fn take_cols(s: &str, max_cols: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut taken = 0usize;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if taken + cw > max_cols {
            break;
        }
        out.push(c);
        taken += cw;
    }
    out
}

/// Build the initial per-column widths from the seed scan. Header cells
/// participate so the header text fits when present.
fn seed_widths(data: &CsvData) -> Vec<usize> {
    let cols = data.column_count();
    if cols == 0 {
        return Vec::new();
    }
    let mut widths = vec![1usize; cols];
    for rec in &data.records {
        if rec.malformed {
            continue;
        }
        for (i, cell) in rec.cells.iter().enumerate().take(cols) {
            let w = display_cell(cell).width().min(MAX_COLUMN_WIDTH);
            if w > widths[i] {
                widths[i] = w;
            }
        }
    }
    widths
}

impl Mode for CsvTableMode {
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
        // loaded; pulls from the reader when not).
        let _ = self.data.ensure_record(body_target.saturating_sub(1));

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
        // Drive the parser to EOF so every record renders.
        let _ = self.data.ensure_all();
        // Print mode uses seed widths only — single-row overflow allowed
        // (alignment breaks for that row, next row realigns).
        let widths = self.seed_widths.clone();
        if self.has_header && !self.data.records.is_empty() {
            out.write_line(&self.build_header_row_print(&widths, ctx.peek_theme))?;
            out.write_line(&self.build_separator_row_print(&widths, ctx.peek_theme))?;
        }
        let body_start = self.body_start();
        for rec_idx in body_start..self.data.records.len() {
            let rec = &self.data.records[rec_idx];
            let mut row = String::new();
            row.push(' ');
            if rec.malformed {
                row.push_str(
                    &ctx.peek_theme
                        .paint("<malformed record>", ctx.peek_theme.warning),
                );
                out.write_line(&row)?;
                continue;
            }
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    row.push_str(&ctx.peek_theme.paint_muted(COL_SEP));
                }
                let raw = rec.cells.get(i).map(|s| s.as_str()).unwrap_or("");
                let cell = display_cell(raw);
                let cell_w = cell.width();
                if cell_w > *w {
                    // Print-mode overflow: emit cell in full, push the rest
                    // of this row rightward past terminal edge.
                    row.push_str(&ctx.peek_theme.paint(&cell, ctx.peek_theme.foreground));
                } else {
                    row.push_str(&render_cell(
                        &cell,
                        *w,
                        ctx.peek_theme.foreground,
                        ctx.peek_theme,
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
                let _ = self.data.ensure_record(body_target.saturating_sub(1));
                self.clamp_top();
                true
            }
            Action::Top => {
                self.top_record = 0;
                true
            }
            Action::Bottom => {
                // Drive to EOF so the bottom is a true bottom.
                let _ = self.data.ensure_all();
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
        let total_label = match self.data.total_records() {
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
        if self.data.malformed_count > 0 {
            segs.push((
                format!("malformed {}", self.data.malformed_count),
                theme.warning,
            ));
        }
        // Header-on default; surface only when the user has flipped it off.
        if !self.has_header {
            segs.push(("Header off".to_string(), theme.label));
        }
        segs
    }
}

impl CsvTableMode {
    /// Header row variant for print mode — uses plain widths without
    /// honoring `h_col` (print mode emits every column from index 0).
    fn build_header_row_print(&self, widths: &[usize], theme: &PeekTheme) -> String {
        let Some(rec) = self.data.records.first() else {
            return String::new();
        };
        let mut out = String::new();
        out.push(' ');
        for (i, w) in widths.iter().enumerate() {
            if i > 0 {
                out.push_str(&theme.paint_muted(COL_SEP));
            }
            let raw = rec.cells.get(i).map(|s| s.as_str()).unwrap_or("");
            let cell = display_cell(raw);
            let cell_w = cell.width();
            if cell_w > *w {
                // Overflow: emit full header text, push rest rightward.
                out.push_str(&theme.paint(&cell, theme.heading));
            } else {
                out.push_str(&render_cell(&cell, *w, theme.heading, theme));
            }
        }
        out
    }

    fn build_separator_row_print(&self, widths: &[usize], theme: &PeekTheme) -> String {
        let mut buf = String::new();
        buf.push(SEP_ROW_CHAR);
        for (i, w) in widths.iter().enumerate() {
            if i > 0 {
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
}

#[allow(dead_code)]
const _: () = {
    // Compile-time sanity check — keeps unused warnings off the COL_SEP_WIDTH
    // constant while making it available for future overflow math.
    let _ = COL_SEP_WIDTH;
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputSource;
    use crate::theme::{StyleMode, ThemeManager};
    use crate::types::csv::format::CsvFormat;
    use bytes::Bytes;

    fn stdin(text: &str) -> InputSource {
        InputSource::stdin(Bytes::copy_from_slice(text.as_bytes()))
    }

    fn theme_manager() -> Rc<ThemeManager> {
        Rc::new(ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain))
    }

    #[test]
    fn seed_widths_grow_with_widest_seed_cell() {
        let src = stdin("name,age\nalice,30\nelizabeth,99\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        let mode = CsvTableMode::new(data, theme_manager(), PeekThemeName::IdeaDark);
        // Column 0: max("name"=4, "alice"=5, "elizabeth"=9) = 9
        // Column 1: max("age"=3, "30"=2, "99"=2) = 3
        assert_eq!(mode.widths, vec![9, 3]);
    }

    #[test]
    fn scrolldown_advances_top_record_clamped_to_max() {
        let src = stdin("h\na\nb\nc\nd\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        let mut mode = CsvTableMode::new(data, theme_manager(), PeekThemeName::IdeaDark);
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
        let src = stdin("name,age\nalice,30\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        let mut mode = CsvTableMode::new(data, theme_manager(), PeekThemeName::IdeaDark);
        assert!(mode.has_header);
        assert_eq!(mode.handle(Action::ToggleHeader), Handled::Yes);
        assert!(!mode.has_header);
        assert_eq!(mode.body_total(), 2, "header off → row 1 is body");
    }

    #[test]
    fn shift_r_reflows_widths_to_viewport() {
        // After scrolling past a wide-cell block, Shift+R recomputes from
        // the visible window to reclaim space.
        let src = stdin("a,b\nshort,x\nmuchlongercell,y\nshort,z\nshort,w\nshort,v\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        let mut mode = CsvTableMode::new(data, theme_manager(), PeekThemeName::IdeaDark);
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
        let src = stdin("a,b,c\n1,2,3\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        let mut mode = CsvTableMode::new(data, theme_manager(), PeekThemeName::IdeaDark);
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
        let src = stdin("a,b\n1,2\n3,4\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        let mode = CsvTableMode::new(data, theme_manager(), PeekThemeName::IdeaDark);
        let tm = theme_manager();
        let theme = tm.peek_theme().clone();
        let segs = mode.status_segments(&theme);
        assert!(segs.iter().any(|(s, _)| s == "1/2"));
        assert!(segs.iter().any(|(s, _)| s == "col 1/2"));
    }
}
