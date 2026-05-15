//! CSV / TSV info section shape.
//!
//! Per-column type inference comes from the seed sample (up to
//! `SEED_RECORD_LIMIT` records); the `sampled` flag stays true when the
//! file is larger than the seed window so the rendered info row carries
//! the qualifier.

use super::format::CsvFormat;
use super::parse::CellKind;

#[derive(Debug, Clone)]
pub struct CsvStats {
    pub format: CsvFormat,
    pub delimiter: u8,
    pub encoding: &'static str,
    pub has_bom: bool,
    pub header_detected: bool,
    pub columns: Vec<ColumnStats>,
    /// Number of records the parser has loaded so far (≥ records when
    /// `total_records` is `Some`).
    pub loaded_records: usize,
    /// `Some(n)` when the parser has been driven to EOF (every record
    /// counted). `None` while only a partial scan has run.
    pub total_records: Option<usize>,
    pub malformed_count: usize,
    /// True when the type inference / per-column stats were computed
    /// from a sample (not the full file).
    pub sampled: bool,
}

#[derive(Debug, Clone)]
pub struct ColumnStats {
    pub header: Option<String>,
    pub inferred_type: ColumnType,
    pub empty_count: usize,
    pub max_width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Int,
    Float,
    Bool,
    Date,
    String,
    /// Mixed types observed across the sample (after excluding empty cells).
    Mixed,
}

impl ColumnType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::Date => "date",
            Self::String => "string",
            Self::Mixed => "mixed",
        }
    }

    pub(super) fn from_kind(kind: CellKind) -> Self {
        match kind {
            CellKind::Int => Self::Int,
            CellKind::Float => Self::Float,
            CellKind::Bool => Self::Bool,
            CellKind::Date => Self::Date,
            CellKind::Text => Self::String,
            CellKind::Empty => Self::String,
        }
    }

    pub(super) fn merge(a: Self, b: Self) -> Self {
        if a == b {
            return a;
        }
        // int + float → float
        if matches!((a, b), (Self::Int, Self::Float) | (Self::Float, Self::Int)) {
            return Self::Float;
        }
        Self::Mixed
    }
}

pub fn delimiter_label(d: u8) -> &'static str {
    match d {
        b',' => "comma",
        b'\t' => "tab",
        b';' => "semicolon",
        b'|' => "pipe",
        _ => "other",
    }
}
