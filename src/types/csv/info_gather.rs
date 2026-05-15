//! Build `CsvStats` from a `CsvData` seed scan.

use super::format::CsvFormat;
use super::info::{ColumnStats, ColumnType, CsvStats};
use super::parse::{CellKind, CsvData, SEED_RECORD_LIMIT, classify_cell};

/// Match the table view's display-collapse so the reported max width
/// equals the rendered column width (no `\n` inflation).
fn display_width(s: &str) -> usize {
    let mut w = 0usize;
    for c in s.chars() {
        let cw = match c {
            '\n' => unicode_width::UnicodeWidthChar::width('\u{21B5}').unwrap_or(1),
            '\r' => 0,
            '\t' => 1,
            _ => unicode_width::UnicodeWidthChar::width(c).unwrap_or(0),
        };
        w += cw;
    }
    w
}

pub fn gather(data: &CsvData, fmt: CsvFormat) -> CsvStats {
    let col_count = data.column_count();
    let mut columns: Vec<ColumnStats> = (0..col_count)
        .map(|_| ColumnStats {
            header: None,
            inferred_type: ColumnType::String,
            empty_count: 0,
            max_width: 0,
        })
        .collect();

    let body_start = if data.header_heuristic { 1 } else { 0 };

    // Capture header cells when the heuristic flagged row 0 as a header.
    if data.header_heuristic
        && let Some(first) = data.records.first()
    {
        for (i, cell) in first.cells.iter().enumerate() {
            if let Some(col) = columns.get_mut(i) {
                col.header = Some(cell.clone());
                col.max_width = display_width(cell);
            }
        }
    }

    // Walk body rows from the seed.
    let mut col_type: Vec<Option<ColumnType>> = vec![None; col_count];
    for record in data.records.iter().skip(body_start) {
        if record.malformed {
            continue;
        }
        for (i, cell) in record.cells.iter().enumerate() {
            if i >= col_count {
                break;
            }
            let w = display_width(cell);
            if w > columns[i].max_width {
                columns[i].max_width = w;
            }
            let kind = classify_cell(cell);
            if matches!(kind, CellKind::Empty) {
                columns[i].empty_count += 1;
            } else {
                let proposed = ColumnType::from_kind(kind);
                col_type[i] = Some(match col_type[i] {
                    None => proposed,
                    Some(prev) => ColumnType::merge(prev, proposed),
                });
            }
        }
    }
    for (i, col) in columns.iter_mut().enumerate() {
        col.inferred_type = col_type[i].unwrap_or(ColumnType::String);
    }

    let sampled = data.total_records().is_none() && data.records.len() >= SEED_RECORD_LIMIT;

    CsvStats {
        format: fmt,
        delimiter: data.delimiter,
        encoding: data.encoding.label(),
        has_bom: data.has_bom,
        header_detected: data.header_heuristic,
        columns,
        loaded_records: data.loaded(),
        total_records: data.total_records(),
        malformed_count: data.malformed_count,
        sampled,
    }
}
