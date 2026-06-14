//! The CSV / TSV info section: a format block plus a per-column block, driven
//! by one [`CsvView`] that derives both `serde::Serialize` (JSON) and
//! [`InfoView`](crate::info::InfoView) (themed print). [`CsvStats`] stays the
//! gather struct; the view projects it.
//!
//! JSON stays flat (both blocks flatten into one object). The column list
//! prints as a `Columns` block with one dynamically-labelled row per column
//! (` 1: header`) and serializes as a `columns` array.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use serde_json::json;

use crate::info::{
    Accent, InfoNode, InfoValue, Role, Value, paint_count, render_info, thousands_sep,
};
use peek_theme::PeekTheme;

use super::info::{ColumnStats, ColumnType, CsvStats, delimiter_label};
use peek_detect::CsvFormat;

/// Themed terminal CSV section (format + Columns blocks).
pub fn render_section(lines: &mut Vec<String>, stats: &CsvStats, theme: &PeekTheme) {
    render_info(lines, &CsvView::from(stats), theme);
}

/// Typed `--info --json` view of the CSV section, nested under `"csv"`.
pub fn json_section(stats: &CsvStats) -> (&'static str, serde_json::Value) {
    (
        "csv",
        serde_json::to_value(CsvView::from(stats)).expect("csv info view serializes"),
    )
}

#[derive(Serialize, crate::info::InfoView)]
struct CsvView {
    #[info(nest)]
    #[serde(flatten)]
    main: CsvMain,
    #[info(nest)]
    #[serde(flatten)]
    columns: Columns,
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title_from = "section_title")]
struct CsvMain {
    #[info(skip)]
    #[serde(skip)]
    format: CsvFormat,
    #[info(label = "Delimiter")]
    delimiter: Accent,
    #[info(label = "Encoding")]
    encoding: &'static str,
    #[info(label = "BOM", skip_if = "Bom::clear")]
    has_bom: Bom,
    #[info(label = "Header")]
    header_detected: Value,
    #[info(label = "Records")]
    #[serde(flatten)]
    records: Records,
    #[info(label = "Columns")]
    column_count: Value,
    #[info(label = "Malformed", skip_if_zero)]
    malformed_count: Value,
    #[info(skip)]
    sampled: bool,
}

impl CsvMain {
    fn section_title(&self) -> &'static str {
        self.format.label()
    }
}

impl From<&CsvStats> for CsvView {
    fn from(s: &CsvStats) -> Self {
        CsvView {
            main: CsvMain {
                format: s.format,
                delimiter: Accent(delimiter_label(s.delimiter).to_string()),
                encoding: s.encoding,
                has_bom: Bom(s.has_bom),
                header_detected: Value::split(
                    if s.header_detected {
                        "detected"
                    } else {
                        "none"
                    },
                    Role::Value,
                    json!(s.header_detected),
                ),
                records: Records {
                    loaded: s.loaded_records,
                    total: s.total_records,
                },
                column_count: Value::count(s.columns.len() as u64),
                malformed_count: Value::split(
                    thousands_sep(s.malformed_count as u64),
                    Role::Warn,
                    json!(s.malformed_count),
                ),
                sampled: s.sampled,
            },
            columns: Columns {
                sampled: s.sampled,
                loaded_records: s.loaded_records,
                columns: s.columns.clone(),
            },
        }
    }
}

/// BOM flag: JSON bool always, print a muted `yes` only when present.
struct Bom(bool);
impl Bom {
    fn clear(&self) -> bool {
        !self.0
    }
}
impl InfoValue for Bom {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_muted("yes")
    }
}
impl Serialize for Bom {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_bool(self.0)
    }
}

/// Record count. Print: the exact total, or `loaded (partial)` while still
/// sampling. JSON: `loaded_records` + optional `total_records`.
struct Records {
    loaded: usize,
    total: Option<usize>,
}
impl InfoValue for Records {
    fn render_value(&self, theme: &PeekTheme) -> String {
        match self.total {
            Some(n) => paint_count(n, theme),
            None => format!(
                "{} {}",
                paint_count(self.loaded, theme),
                theme.paint_muted("(partial)")
            ),
        }
    }
}
impl Serialize for Records {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let len = if self.total.is_some() { 2 } else { 1 };
        let mut st = ser.serialize_struct("records", len)?;
        st.serialize_field("loaded_records", &self.loaded)?;
        if let Some(n) = self.total {
            st.serialize_field("total_records", &n)?;
        }
        st.end()
    }
}

/// Per-column block. Print: a `Columns` section with an optional `Sample` row
/// then one dynamically-labelled row per column. JSON: a `columns` array.
struct Columns {
    sampled: bool,
    loaded_records: usize,
    columns: Vec<ColumnStats>,
}

impl crate::info::InfoView for Columns {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        if self.columns.is_empty() {
            return Vec::new();
        }
        let mut body = Vec::new();
        if self.sampled {
            body.push(InfoNode::Row {
                label: "Sample".into(),
                value: theme.paint_muted(&format!("first {} records", self.loaded_records)),
            });
        }
        for (idx, col) in self.columns.iter().enumerate() {
            let header = col.header.as_deref().unwrap_or("(no header)");
            let value = format!(
                "{}  width {}  empty {}",
                theme.paint_accent(col.inferred_type.label()),
                theme.paint_value(&col.max_width.to_string()),
                theme.paint_muted(&col.empty_count.to_string()),
            );
            body.push(InfoNode::Row {
                label: format!("{:>2}: {header}", idx + 1).into(),
                value,
            });
        }
        vec![InfoNode::Block {
            title: "Columns".to_string(),
            body,
        }]
    }
}

impl Serialize for Columns {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let entries: Vec<ColEntry> = self.columns.iter().map(ColEntry::from).collect();
        let mut st = ser.serialize_struct("columns", 1)?;
        st.serialize_field("columns", &entries)?;
        st.end()
    }
}

#[derive(Serialize)]
struct ColEntry {
    #[serde(serialize_with = "ser_coltype")]
    inferred_type: ColumnType,
    empty_count: usize,
    max_width: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    header: Option<String>,
}

impl From<&ColumnStats> for ColEntry {
    fn from(c: &ColumnStats) -> Self {
        ColEntry {
            inferred_type: c.inferred_type,
            empty_count: c.empty_count,
            max_width: c.max_width,
            header: c.header.clone(),
        }
    }
}

fn ser_coltype<S: Serializer>(t: &ColumnType, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(t.label())
}
