//! `calamine` workbook wrapper + per-sheet [`RowSource`].
//!
//! Opening lists sheet names cheaply (workbook metadata only). A sheet
//! is materialised on demand — `calamine` has no streaming sheet API, so
//! `worksheet_range` parses the whole `sheetN.xml` into an in-memory
//! `Range`. Resident memory is therefore one sheet at a time; drilling
//! into another sheet (a fresh `DescendFrame`) drops the prior one.
//! Sheets are bounded (Excel caps at ~1M rows; real ones far smaller),
//! so this is the accepted trade — a truly streaming xlsx reader would
//! need a custom SAX parse of the sheet XML.

use std::io::Cursor;

use anyhow::{Result, anyhow};
use bytes::Bytes;
use calamine::{Data, Ods, Reader, Sheets, Xlsx};

use crate::input::InputSource;
use crate::viewer::table::row_source::RowSource;
use crate::viewer::table::rows_mode::Alignment;

use super::SpreadsheetFormat;

pub(crate) struct Workbook {
    sheets: Sheets<Cursor<Bytes>>,
}

impl Workbook {
    pub fn open(source: &InputSource, fmt: SpreadsheetFormat) -> Result<Self> {
        // A workbook is a ZIP container: its central directory lives at
        // the end, so calamine needs random access over the whole file.
        // Read it into a refcounted `Bytes` cursor (cheap to hand to
        // calamine, no per-attempt copy). The container is small; the
        // memory that matters is the materialised sheet, not this.
        let cursor = Cursor::new(source.read_bytes()?);
        // Dispatch to the concrete reader by detected format.
        let sheets = if fmt.is_ooxml() {
            Sheets::Xlsx(Xlsx::new(cursor).map_err(|e| anyhow!("failed to open workbook: {e:?}"))?)
        } else {
            Sheets::Ods(Ods::new(cursor).map_err(|e| anyhow!("failed to open workbook: {e:?}"))?)
        };
        Ok(Self { sheets })
    }

    /// Sheet names in workbook order. Cheap — reads container metadata
    /// only, not cell data.
    pub fn sheet_names(&self) -> Vec<String> {
        self.sheets.sheet_names()
    }

    /// Materialise one sheet into a [`Sheet`] row source.
    pub fn materialize(&mut self, name: &str) -> Result<Sheet> {
        let range = self
            .sheets
            .worksheet_range(name)
            .map_err(|e| anyhow!("reading sheet {name}: {e:?}"))?;
        Ok(Sheet::from_range(&range))
    }
}

/// One materialised sheet, ready to drive a `RowsTableMode`.
pub(crate) struct Sheet {
    rows: Vec<Vec<Option<String>>>,
    cols: usize,
    aligns: Vec<Alignment>,
    /// Row 0 looks like a header (all text / empty) — drives the sticky
    /// header, same heuristic as the CSV viewer (`Shift+H` overrides).
    header: bool,
    empty: Vec<Option<String>>,
}

impl Sheet {
    fn from_range(range: &calamine::Range<Data>) -> Self {
        let cols = range.width();
        let height = range.height();

        // Header heuristic: row 0 present and every cell text-shaped
        // (String / Empty — no number, bool, or date).
        let header = cols > 0
            && range.rows().next().is_some_and(|r| {
                (0..cols).all(|c| {
                    matches!(
                        r.get(c).unwrap_or(&Data::Empty),
                        Data::Empty | Data::String(_)
                    )
                })
            });
        let body_start = header as usize;

        let mut rows: Vec<Vec<Option<String>>> = Vec::with_capacity(height);
        // Per-column numeric tracking for alignment, from body rows only
        // (the header's text labels must not veto a numeric column).
        let mut numeric = vec![true; cols];
        let mut any_typed = vec![false; cols];

        for (r, row) in range.rows().enumerate() {
            let mut cells = Vec::with_capacity(cols);
            for c in 0..cols {
                let datum = row.get(c).unwrap_or(&Data::Empty);
                if r >= body_start {
                    match datum {
                        Data::Empty => {}
                        Data::Int(_) | Data::Float(_) => any_typed[c] = true,
                        _ => numeric[c] = false,
                    }
                }
                cells.push(cell_string(datum));
            }
            rows.push(cells);
        }

        let aligns = (0..cols)
            .map(|c| {
                if numeric[c] && any_typed[c] {
                    Alignment::Right
                } else {
                    Alignment::Left
                }
            })
            .collect();

        Self {
            rows,
            cols,
            aligns,
            header,
            empty: Vec::new(),
        }
    }

    pub fn alignments(&self) -> Vec<Alignment> {
        self.aligns.clone()
    }

    pub fn header_detected(&self) -> bool {
        self.header
    }

    /// All rows (header included) for a CSV dump.
    pub fn csv_rows(&self) -> &[Vec<Option<String>>] {
        &self.rows
    }
}

impl RowSource for Sheet {
    fn ensure_row(&mut self, _idx: usize) -> Result<usize> {
        Ok(self.rows.len())
    }

    fn ensure_all(&mut self) -> Result<()> {
        Ok(())
    }

    fn row(&self, idx: usize) -> Option<&[Option<String>]> {
        if self.cols == 0 {
            return Some(&self.empty);
        }
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

/// `calamine` cell → display string. Empty → `None` (rendered distinct
/// from an empty string, matching the SQLite NULL convention); every
/// other variant stringifies through `Data`'s `Display`, which gives
/// round-trip-shortest floats and the ISO forms for dates / durations.
fn cell_string(d: &Data) -> Option<String> {
    match d {
        Data::Empty => None,
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(rel: &str) -> InputSource {
        let mut p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        p.push(rel);
        InputSource::File(p)
    }

    fn check_people(fmt: SpreadsheetFormat, rel: &str) {
        let mut wb = Workbook::open(&fixture(rel), fmt).expect("open workbook");
        let names = wb.sheet_names();
        assert_eq!(names, vec!["people".to_string(), "totals".to_string()]);

        let sheet = wb.materialize("people").expect("materialize people");
        assert_eq!(sheet.column_count(), 5);
        assert!(sheet.header_detected(), "row 0 is the column-name header");

        // Header row.
        let header: Vec<&str> = sheet.row(0).unwrap().iter().map(opt).collect();
        assert_eq!(header, ["name", "age", "score", "active", "joined"]);

        // First data row carries the typed values.
        let alice: Vec<&str> = sheet.row(1).unwrap().iter().map(opt).collect();
        assert_eq!(alice[0], "Alice");
        assert_eq!(alice[1], "30");

        // Alignment from native cell types: age + score numeric → right;
        // name / active / joined → left.
        let a = sheet.alignments();
        assert_eq!(a[0], Alignment::Left, "name");
        assert_eq!(a[1], Alignment::Right, "age");
        assert_eq!(a[2], Alignment::Right, "score");
        assert_eq!(a[3], Alignment::Left, "active (bool)");
        assert_eq!(a[4], Alignment::Left, "joined (date)");
    }

    fn opt(c: &Option<String>) -> &str {
        c.as_deref().unwrap_or("")
    }

    #[test]
    fn reads_xlsx_people_sheet() {
        check_people(SpreadsheetFormat::Xlsx, "test-data/people.xlsx");
    }

    #[test]
    fn reads_ods_people_sheet() {
        check_people(SpreadsheetFormat::Ods, "test-data/people.ods");
    }
}
