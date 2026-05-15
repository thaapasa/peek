//! CSV / TSV format flavours. Drives delimiter choice and the label
//! shown in the Format info row.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvFormat {
    /// Comma-separated values (`.csv`).
    Csv,
    /// Tab-separated values (`.tsv`).
    Tsv,
}

impl CsvFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Tsv => "TSV",
        }
    }

    /// Default delimiter byte for the extension-based classification.
    /// Content-sniffing in `parse::sniff_delimiter` may override this for
    /// misnamed files (e.g. a `.csv` that's actually tab-separated).
    pub fn default_delimiter(self) -> u8 {
        match self {
            Self::Csv => b',',
            Self::Tsv => b'\t',
        }
    }
}
