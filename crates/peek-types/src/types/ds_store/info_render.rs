//! The `.DS_Store` info section, driven by one [`DsStoreView`] that
//! derives both `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView)
//! (themed print). [`DsStoreInfo`] stays the gather struct; the view
//! projects it. On a parse error only the `Status` row shows (JSON: an
//! `error` key).

use serde::Serialize;

use super::info::DsStoreInfo;
use crate::info::{Value, Warn, render_info};
use crate::theme::PeekTheme;

/// Themed terminal `.DS_Store` section.
pub fn render_section(lines: &mut Vec<String>, info: &DsStoreInfo, theme: &PeekTheme) {
    render_info(lines, &DsStoreView::from(info), theme);
}

/// Typed `--info --json` view of the section, nested under `"ds_store"`.
pub fn json_section(info: &DsStoreInfo) -> (&'static str, serde_json::Value) {
    (
        "ds_store",
        serde_json::to_value(DsStoreView::from(info)).expect("ds_store info view serializes"),
    )
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "Desktop Services Store")]
struct DsStoreView {
    // Parse failure: print shows a warning Status row; JSON an `error` key.
    #[info(label = "Status", skip_if = "Option::is_none")]
    #[serde(skip)]
    status: Option<Warn>,
    #[info(skip)]
    #[serde(rename = "error", skip_serializing_if = "Option::is_none")]
    error: Option<String>,

    #[info(label = "Records")]
    #[serde(rename = "record_count", skip_serializing_if = "Option::is_none")]
    record_count: Option<Value>,
    #[info(label = "Tracked files")]
    #[serde(rename = "file_count", skip_serializing_if = "Option::is_none")]
    file_count: Option<Value>,
    #[info(label = "View style")]
    #[serde(rename = "view_style", skip_serializing_if = "Option::is_none")]
    view_style: Option<String>,
    #[info(label = "Background")]
    #[serde(skip_serializing_if = "Option::is_none")]
    background: Option<String>,
    // Print: a warning Note row when the walk stopped early. JSON: a plain
    // `truncated` boolean.
    #[info(label = "Note", skip_if = "Option::is_none")]
    #[serde(skip)]
    note: Option<Warn>,
    #[info(skip)]
    #[serde(rename = "truncated")]
    truncated: bool,
}

impl From<&DsStoreInfo> for DsStoreView {
    fn from(info: &DsStoreInfo) -> Self {
        let Some(meta) = &info.meta else {
            return DsStoreView {
                status: Some(Warn(
                    info.error
                        .clone()
                        .unwrap_or_else(|| "could not parse .DS_Store".to_string()),
                )),
                error: info.error.clone(),
                record_count: None,
                file_count: None,
                view_style: None,
                background: None,
                note: None,
                truncated: false,
            };
        };
        DsStoreView {
            status: None,
            error: None,
            record_count: Some(Value::count(meta.record_count as u64)),
            file_count: Some(Value::count(meta.file_count as u64)),
            view_style: meta.view_style.clone(),
            background: meta.background.clone(),
            note: meta
                .truncated
                .then(|| Warn("parse stopped early — some records may be missing".to_string())),
            truncated: meta.truncated,
        }
    }
}
