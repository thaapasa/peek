//! `SheetListSource`: a workbook's sheets as a flat listing. A sheet has a
//! name and nothing file-shaped — no size, no mtime — so this shows just
//! the name (cleanly, without the internal `.csv` key suffix). Selecting a
//! row drills into a streaming table view via the engine's descend handler
//! (`spreadsheet::compose`); the extract key keeps the `.csv` suffix the
//! handler and extractor key off.

use peek_theme::PeekTheme;

use super::compose::SHEET_SUFFIX;
use crate::viewer::listing::{ListSource, ListingHelp, NameCell, RowCells};
use crate::viewer::modes::{ExtractTarget, RenderCtx};

pub struct SheetListSource {
    /// Sheet names, as displayed (no suffix).
    names: Vec<String>,
    /// Status-segment label ("XLSX" / "ODS" / …).
    label: String,
}

impl SheetListSource {
    pub fn new(names: &[String], label: impl Into<String>) -> Self {
        Self {
            names: names.to_vec(),
            label: label.into(),
        }
    }
}

impl ListSource for SheetListSource {
    fn len(&self) -> usize {
        self.names.len()
    }

    fn parent(&self, _idx: usize) -> Option<usize> {
        None
    }

    fn selectable(&self, _idx: usize) -> bool {
        true
    }

    fn name(&self, idx: usize) -> &str {
        &self.names[idx]
    }

    fn source_label(&self) -> &str {
        &self.label
    }

    /// The extract / descend key carries the `.csv` suffix the handler and
    /// the sheet extractor use to tell a sheet apart from a raw zip path.
    fn extract_target(&self, idx: usize) -> Option<ExtractTarget> {
        Some(ExtractTarget::EntryPath(format!(
            "{}{SHEET_SUFFIX}",
            self.names[idx]
        )))
    }

    /// Flat — no parents, so no sticky toggle to advertise.
    fn help(&self) -> ListingHelp {
        ListingHelp {
            sticky: false,
            ..Default::default()
        }
    }

    fn row_cells(&self, idx: usize, _ctx: &RenderCtx) -> RowCells {
        RowCells {
            prefix: String::new(),
            left: Vec::new(),
            name: NameCell {
                text: self.names[idx].clone(),
                is_dir: false,
            },
        }
    }

    /// `--list` emits the extract key (with suffix) so it pipes straight
    /// into `--extract`.
    fn flat_line(&self, idx: usize, theme: &PeekTheme) -> Option<String> {
        let name = peek_io::sanitize_terminal_controls(&self.names[idx]);
        Some(theme.paint(&format!("{name}{SHEET_SUFFIX}"), theme.foreground))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src() -> SheetListSource {
        SheetListSource::new(&["Budget".to_string(), "Q1".to_string()], "XLSX")
    }

    #[test]
    fn names_shown_without_suffix() {
        let s = src();
        assert_eq!(s.name(0), "Budget");
        assert_eq!(s.name(1), "Q1");
    }

    #[test]
    fn extract_key_keeps_suffix() {
        match src().extract_target(0) {
            Some(ExtractTarget::EntryPath(p)) => assert_eq!(p, "Budget.csv"),
            other => panic!("expected EntryPath, got {other:?}"),
        }
    }

    #[test]
    fn all_rows_flat_and_selectable() {
        let s = src();
        assert!((0..s.len()).all(|i| s.selectable(i) && s.parent(i).is_none()));
    }
}
