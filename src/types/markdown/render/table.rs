//! GFM table rendering: rows of styled cell strings → ASCII
//! box-drawing rows that fit the terminal width.
//!
//! Column widths are sized to the widest cell per column, then scaled
//! down proportionally when the joined row would exceed the available
//! width. Cells wrap at their column width with the active SGR style
//! preserved across cuts.

use pulldown_cmark::Alignment;

use crate::theme::PeekTheme;
use crate::viewer::ui::wrap_styled;

use super::wrap::display_width;

const MIN_COL_WIDTH: usize = 3;
const CELL_PAD: usize = 1;

/// Render a parsed table into styled box-drawing rows. `available` is
/// the body width after the row prefix (blockquote rail / list indent)
/// is accounted for.
pub(super) fn render(
    head: &[Vec<String>],
    body: &[Vec<String>],
    alignments: &[Alignment],
    available: usize,
    theme: &PeekTheme,
) -> Vec<String> {
    let cols = head
        .first()
        .map(|r| r.len())
        .or_else(|| body.first().map(|r| r.len()))
        .unwrap_or(0);
    if cols == 0 {
        return Vec::new();
    }

    let aligns: Vec<Alignment> = (0..cols)
        .map(|i| alignments.get(i).copied().unwrap_or(Alignment::None))
        .collect();
    let widths = compute_widths(head, body, cols, available);

    let mut out = Vec::new();
    out.push(border_row(&widths, BorderKind::Top, theme));
    for row in head {
        for line in cell_lines(row, &widths, &aligns, theme) {
            out.push(line);
        }
        out.push(border_row(&widths, BorderKind::HeadSep, theme));
    }
    for (i, row) in body.iter().enumerate() {
        if i > 0 {
            // No inter-row separators — keeps tall tables readable.
            // The vertical pipes alone delineate rows.
        }
        for line in cell_lines(row, &widths, &aligns, theme) {
            out.push(line);
        }
    }
    out.push(border_row(&widths, BorderKind::Bottom, theme));
    out
}

fn compute_widths(
    head: &[Vec<String>],
    body: &[Vec<String>],
    cols: usize,
    available: usize,
) -> Vec<usize> {
    let mut widths = vec![MIN_COL_WIDTH; cols];
    for row in head.iter().chain(body.iter()) {
        for (i, cell) in row.iter().enumerate().take(cols) {
            let w = display_width(cell).max(MIN_COL_WIDTH);
            if w > widths[i] {
                widths[i] = w;
            }
        }
    }
    // Total = vertical bars (cols+1) + per-cell padding (2*cols) + body widths.
    let chrome = cols + 1 + 2 * CELL_PAD * cols;
    let body_budget = available.saturating_sub(chrome).max(cols * MIN_COL_WIDTH);
    let sum: usize = widths.iter().sum();
    if sum > body_budget {
        // Proportional shrink, floor at MIN_COL_WIDTH.
        let mut shrunk: Vec<usize> = widths
            .iter()
            .map(|&w| ((w as f64 * body_budget as f64 / sum as f64) as usize).max(MIN_COL_WIDTH))
            .collect();
        // Adjust rounding drift so the joined width matches exactly.
        let drift = shrunk.iter().sum::<usize>() as isize - body_budget as isize;
        if drift > 0 {
            for w in shrunk.iter_mut() {
                if drift == 0 {
                    break;
                }
                if *w > MIN_COL_WIDTH {
                    *w -= 1;
                }
            }
        }
        widths = shrunk;
    }
    widths
}

#[derive(Clone, Copy)]
enum BorderKind {
    Top,
    HeadSep,
    Bottom,
}

fn border_row(widths: &[usize], kind: BorderKind, theme: &PeekTheme) -> String {
    let (left, mid, right) = match kind {
        BorderKind::Top => ('┌', '┬', '┐'),
        BorderKind::HeadSep => ('├', '┼', '┤'),
        BorderKind::Bottom => ('└', '┴', '┘'),
    };
    let mut s = String::new();
    s.push(left);
    for (i, w) in widths.iter().enumerate() {
        s.extend(std::iter::repeat_n('─', w + 2 * CELL_PAD));
        s.push(if i + 1 == widths.len() { right } else { mid });
    }
    theme.paint_muted(&s)
}

fn cell_lines(
    row: &[String],
    widths: &[usize],
    aligns: &[Alignment],
    theme: &PeekTheme,
) -> Vec<String> {
    // Wrap each cell to its column width and pad short cells with empty
    // continuation rows so all cells in the row produce the same number
    // of visual lines.
    let wrapped: Vec<Vec<String>> = (0..widths.len())
        .map(|i| {
            let cell = row.get(i).map(String::as_str).unwrap_or("");
            wrap_styled(cell, widths[i])
        })
        .collect();
    let row_height = wrapped.iter().map(|v| v.len()).max().unwrap_or(1);
    let bar = theme.paint_muted("│");
    let mut out = Vec::with_capacity(row_height);
    for r in 0..row_height {
        let mut line = String::new();
        line.push_str(&bar);
        for (i, col) in wrapped.iter().enumerate() {
            let raw = col.get(r).cloned().unwrap_or_default();
            let padded = pad_to(&raw, widths[i], aligns[i]);
            line.push_str(&" ".repeat(CELL_PAD));
            line.push_str(&padded);
            line.push_str(&" ".repeat(CELL_PAD));
            line.push_str(&bar);
        }
        out.push(line);
    }
    out
}

/// Pad / clip a styled string to exactly `width` display columns. SGR
/// escape sequences pass through unchanged; padding is plain spaces.
fn pad_to(s: &str, width: usize, align: Alignment) -> String {
    let w = display_width(s);
    if w >= width {
        return s.to_string();
    }
    let pad = width - w;
    match align {
        Alignment::Right => format!("{}{}", " ".repeat(pad), s),
        Alignment::Center => {
            let l = pad / 2;
            let r = pad - l;
            format!("{}{}{}", " ".repeat(l), s, " ".repeat(r))
        }
        Alignment::Left | Alignment::None => format!("{}{}", s, " ".repeat(pad)),
    }
}
