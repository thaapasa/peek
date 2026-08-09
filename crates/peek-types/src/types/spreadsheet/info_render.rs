//! The spreadsheet workbook info section, driven by one [`SpreadsheetView`]
//! that derives both `serde::Serialize` (JSON) and
//! [`InfoView`](crate::info::InfoView) (themed print). [`SpreadsheetInfo`]
//! stays the gather struct; the view projects it.
//!
//! The sheet list shows two different ways: print gets a `Sheets` count plus a
//! muted `Names` join, JSON gets `sheet_count` plus a `sheets` array — so
//! `names` is a print-only field (`#[serde(skip)]`) and `sheets` a JSON-only
//! field (`#[info(skip)]`). On a load error only the `Error` row shows: the
//! projection blanks every other field.

use serde::{Serialize, Serializer};

use super::SpreadsheetFormat;
use super::info::SpreadsheetInfo;
use crate::info::{Muted, Value};

crate::info_section!(SpreadsheetInfo, SpreadsheetView, "spreadsheet");

#[derive(Serialize, crate::info::InfoView)]
#[info(title_from = "section_title")]
struct SpreadsheetView {
    #[info(skip)]
    #[serde(rename = "format", serialize_with = "ser_format")]
    format: SpreadsheetFormat,
    #[info(label = "Error")]
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<crate::info::Warn>,
    #[info(label = "Sheets")]
    #[serde(rename = "sheet_count", skip_serializing_if = "Option::is_none")]
    sheet_count: Option<Value>,
    // Print-only: the muted name join. JSON carries the array instead.
    #[info(label = "Names", skip_if = "Option::is_none")]
    #[serde(skip)]
    names: Option<Muted>,
    // JSON-only: the sheet names in workbook order.
    #[info(skip)]
    sheets: Vec<String>,
    #[info(label = "Title")]
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[info(label = "Author")]
    #[serde(skip_serializing_if = "Option::is_none")]
    creator: Option<String>,
    #[info(label = "Subject")]
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<Muted>,
    #[info(label = "Keywords")]
    #[serde(skip_serializing_if = "Option::is_none")]
    keywords: Option<Muted>,
    #[info(label = "Created")]
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<Value>,
    #[info(label = "Modified")]
    #[serde(skip_serializing_if = "Option::is_none")]
    modified: Option<Value>,
}

impl SpreadsheetView {
    fn section_title(&self) -> &'static str {
        self.format.label()
    }
}

impl From<&SpreadsheetInfo> for SpreadsheetView {
    fn from(s: &SpreadsheetInfo) -> Self {
        let m = &s.metadata;
        // On error the renderer shows only the Error row; blank the rest.
        let ok = s.error.is_none();
        let muted = |v: &Option<String>| if ok { v.clone().map(Muted) } else { None };
        SpreadsheetView {
            format: s.format,
            error: s.error.clone().map(crate::info::Warn),
            sheet_count: ok.then(|| Value::count(s.sheets.len() as u64)),
            names: (ok && !s.sheets.is_empty()).then(|| Muted(s.sheets.join(", "))),
            sheets: s.sheets.clone(),
            title: if ok { m.title.clone() } else { None },
            creator: if ok { m.creator.clone() } else { None },
            subject: muted(&m.subject),
            keywords: muted(&m.keywords),
            created: ok.then(|| m.created.map(Value::timestamp)).flatten(),
            modified: ok.then(|| m.modified.map(Value::timestamp)).flatten(),
        }
    }
}

fn ser_format<S: Serializer>(format: &SpreadsheetFormat, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(match format {
        SpreadsheetFormat::Xlsx => "xlsx",
        SpreadsheetFormat::Xlsm => "xlsm",
        SpreadsheetFormat::Ods => "ods",
    })
}
