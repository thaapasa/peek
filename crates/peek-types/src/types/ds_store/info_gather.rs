//! `.DS_Store` info gathering: parse the store and summarise it. Parse
//! failures land in `DsStoreInfo::err` so the Info view always renders.

use std::collections::HashSet;

use super::format::view_style_label;
use super::info::{DsStoreInfo, DsStoreMeta};
use super::reader::{self, DsValue};
use crate::info::Extras;
use crate::input::InputSource;

pub fn gather_extras(source: &InputSource) -> Extras {
    Box::new(gather(source))
}

fn gather(source: &InputSource) -> DsStoreInfo {
    let bytes = match source.read_bytes() {
        Ok(b) => b,
        Err(e) => return DsStoreInfo::err(format!("read failed: {e}")),
    };
    let store = match reader::parse(&bytes) {
        Ok(s) => s,
        Err(e) => return DsStoreInfo::err(e.to_string()),
    };

    let file_count = store
        .records
        .iter()
        .map(|r| r.name.as_str())
        .collect::<HashSet<_>>()
        .len();

    // The folder's own view style / background ride a `vstl` / `BKGD`
    // record (keyed against `.` for the folder itself).
    let view_style = store
        .records
        .iter()
        .find_map(|r| match (&r.value, r.code.as_str()) {
            (DsValue::Type(t), "vstl") => Some(view_style_label(t).to_string()),
            _ => None,
        });
    let background = store
        .records
        .iter()
        .find(|r| r.code == "BKGD")
        .map(super::format::format_value);

    DsStoreInfo::ok(DsStoreMeta {
        record_count: store.records.len(),
        file_count,
        view_style,
        background,
        truncated: store.truncated,
    })
}
