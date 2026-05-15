//! Extension-based detection for CSV / TSV files.

use super::format::CsvFormat;

pub fn format_from_ext(ext: &str) -> Option<CsvFormat> {
    match ext {
        "csv" => Some(CsvFormat::Csv),
        "tsv" | "tab" => Some(CsvFormat::Tsv),
        _ => None,
    }
}
