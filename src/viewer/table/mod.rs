//! Generic aligned-table view, shared by the materialised-table file
//! types — object files (Sections / Symbols) and Java classfiles
//! (Fields / Methods).
//!
//! One [`Table`] is a fixed column layout plus rows of typed [`Cell`]s;
//! [`TableMode`] renders it with a sticky header, content-fitted
//! columns, character-offset horizontal scroll, and `/` search.
//!
//! `CsvTableMode` is deliberately *not* built on this — its streaming
//! `CsvData` backing and cell-scoped search are a different mechanism.
//! This mode is for tables fully materialised up front.

mod mode;

pub(crate) use mode::TableMode;

/// One structured table: fixed column layout + rows of typed cells, plus
/// an optional one-line notice shown above the body.
pub(crate) struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Cell>>,
    pub notice: Option<String>,
}

/// One column's static layout. A `width` of 0 marks a flexible last
/// column — rendered at its natural length, never padded or truncated.
pub(crate) struct Column {
    pub header: &'static str,
    pub width: usize,
    pub align: Align,
}

#[derive(Clone, Copy)]
pub(crate) enum Align {
    Left,
    Right,
}

/// One cell: its text plus the colour role the renderer paints it with.
pub(crate) struct Cell {
    pub text: String,
    pub role: CellRole,
    /// Multi-colour cell: when `Some`, the cell is painted span-by-span
    /// (each `(text, role)` pair one token) instead of as a single
    /// `role`-coloured block. `text` mirrors the concatenated span text
    /// so width and search math stay correct.
    pub spans: Option<Vec<(String, CellRole)>>,
}

/// Colour role for a cell — resolved against the live theme at render
/// time so a theme cycle recolours every table. A palette: each file
/// type picks the roles it needs.
#[derive(Clone, Copy)]
pub(crate) enum CellRole {
    /// Index / tag / flag set — theme `label`.
    Tag,
    /// Primary identifier — theme `accent` (keyword colour).
    Primary,
    /// Hex address — `0x` prefix dimmed, digits in the value colour.
    Address,
    /// Numeric quantity — an accent / value blend.
    Numeric,
    /// Secondary / kind text — theme `muted`, plainer than the rest.
    Muted,
    /// Free-text name — plain foreground.
    Name,
}

/// Hard ceiling on a fixed column's width — keeps one pathological cell
/// from pushing the table off-screen; longer cells truncate with `…`.
const MAX_FIXED_WIDTH: usize = 40;

/// Construct a single-colour cell.
pub(crate) fn cell(text: String, role: CellRole) -> Cell {
    Cell {
        text,
        role,
        spans: None,
    }
}

/// Construct a multi-colour cell from styled spans — each `(text, role)`
/// pair is painted as its own token. Used for syntax-highlighted type
/// signatures. The cell's plain `text` is the span concatenation.
pub(crate) fn cell_spans(spans: Vec<(String, CellRole)>) -> Cell {
    let text: String = spans.iter().map(|(t, _)| t.as_str()).collect();
    Cell {
        text,
        role: CellRole::Name,
        spans: Some(spans),
    }
}

/// Build columns whose fixed widths are fitted to the actual cell
/// content (header included). The last column is left flexible
/// (`width == 0`) — rendered at its natural length, panned via the
/// view's horizontal scroll.
pub(crate) fn fit_columns(specs: &[(&'static str, Align)], rows: &[Vec<Cell>]) -> Vec<Column> {
    let last = specs.len().saturating_sub(1);
    specs
        .iter()
        .enumerate()
        .map(|(i, &(header, align))| {
            let width = if i == last {
                0
            } else {
                let widest = rows
                    .iter()
                    .filter_map(|r| r.get(i))
                    .map(|c| c.text.chars().count())
                    .max()
                    .unwrap_or(0);
                widest.max(header.chars().count()).min(MAX_FIXED_WIDTH)
            };
            Column {
                header,
                width,
                align,
            }
        })
        .collect()
}
