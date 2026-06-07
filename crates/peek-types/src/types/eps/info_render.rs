//! The EPS / PostScript info section, driven by one [`EpsView`] that derives
//! both `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView)
//! (themed print). [`EpsInfo`] stays the gather struct; the view projects it.
//!
//! DSC comment fields render inline (and flatten into the JSON object). The
//! `Preview` and `Render` rows always show (the preview falls back to a muted
//! `none`, the renderer to an availability hint), while in JSON the preview is
//! a nested object present only when embedded.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use serde_json::json;

use crate::info::{InfoValue, Muted, Role, Value, format_size_human, render_info};
use crate::theme::PeekTheme;

use super::PostScriptFormat;
use super::dos_eps::PreviewKind;
use super::info::{EpsInfo, PreviewMeta};

/// Themed terminal EPS / PostScript section.
pub fn render_section(lines: &mut Vec<String>, info: &EpsInfo, theme: &PeekTheme) {
    render_info(lines, &EpsView::from(info), theme);
}

/// Typed `--info --json` view of the EPS section, nested under `"eps"`.
pub fn json_section(info: &EpsInfo) -> (&'static str, serde_json::Value) {
    (
        "eps",
        serde_json::to_value(EpsView::from(info)).expect("eps info view serializes"),
    )
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title_from = "section_title")]
struct EpsView {
    #[info(skip)]
    #[serde(rename = "format", serialize_with = "ser_format")]
    format: PostScriptFormat,
    #[info(label = "Title")]
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[info(label = "Creator")]
    #[serde(skip_serializing_if = "Option::is_none")]
    creator: Option<String>,
    #[info(label = "Created")]
    #[serde(rename = "creation_date", skip_serializing_if = "Option::is_none")]
    creation_date: Option<Muted>,
    #[info(label = "For")]
    #[serde(rename = "for_whom", skip_serializing_if = "Option::is_none")]
    for_whom: Option<Muted>,
    #[info(label = "BoundingBox")]
    #[serde(rename = "bounding_box", skip_serializing_if = "Option::is_none")]
    bounding_box: Option<String>,
    #[info(label = "Language")]
    #[serde(rename = "language_level", skip_serializing_if = "Option::is_none")]
    language_level: Option<Muted>,
    #[info(label = "Pages")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pages: Option<Muted>,
    // Always a row (muted `none` when absent); JSON object only when embedded.
    #[info(label = "Preview", no_skip)]
    #[serde(skip_serializing_if = "Preview::is_none")]
    preview: Preview,
    // Always a row; JSON bool.
    #[info(label = "Render")]
    #[serde(rename = "gs_available")]
    render: Value,
}

impl EpsView {
    fn section_title(&self) -> &'static str {
        self.format.label()
    }
}

impl From<&EpsInfo> for EpsView {
    fn from(s: &EpsInfo) -> Self {
        let d = &s.dsc;
        EpsView {
            format: s.format,
            title: d.title.clone(),
            creator: d.creator.clone(),
            creation_date: d.creation_date.clone().map(Muted),
            for_whom: d.for_whom.clone().map(Muted),
            bounding_box: d.bounding_box.clone(),
            language_level: d.language_level.clone().map(Muted),
            pages: d.pages.clone().map(Muted),
            preview: Preview(s.preview.clone()),
            render: if s.gs_available {
                Value::split("Ghostscript", Role::Value, json!(true))
            } else {
                Value::split(
                    "unavailable (install Ghostscript)",
                    Role::Muted,
                    json!(false),
                )
            },
        }
    }
}

/// Embedded preview. Print: a description (`TIFF 64×64 (2 KiB)`) or a muted
/// `none`. JSON: `{ kind, bytes, width?, height? }`, omitted when absent.
struct Preview(Option<PreviewMeta>);

impl Preview {
    fn is_none(&self) -> bool {
        self.0.is_none()
    }
}

impl InfoValue for Preview {
    fn render_value(&self, theme: &PeekTheme) -> String {
        match &self.0 {
            Some(p) => {
                let size = format_size_human(p.bytes as u64);
                let desc = match (p.kind, p.dimensions) {
                    (PreviewKind::Tiff, Some((w, h))) => format!("TIFF {w}×{h} ({size})"),
                    (PreviewKind::Tiff, None) => format!("TIFF ({size}, not rendered)"),
                    (PreviewKind::Wmf, _) => format!("WMF ({size}, not rendered)"),
                };
                theme.paint_value(&desc)
            }
            None => theme.paint_muted("none"),
        }
    }
}

impl Serialize for Preview {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            None => ser.serialize_none(),
            Some(p) => {
                let len = if p.dimensions.is_some() { 4 } else { 2 };
                let mut st = ser.serialize_struct("preview", len)?;
                st.serialize_field("kind", preview_kind_token(p.kind))?;
                st.serialize_field("bytes", &p.bytes)?;
                if let Some((w, h)) = p.dimensions {
                    st.serialize_field("width", &w)?;
                    st.serialize_field("height", &h)?;
                }
                st.end()
            }
        }
    }
}

/// Ghostscript availability. Print: `Ghostscript` or a muted install hint.
/// JSON: a bool.
fn ser_format<S: Serializer>(format: &PostScriptFormat, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(match format {
        PostScriptFormat::Eps => "eps",
        PostScriptFormat::Ps => "ps",
    })
}

fn preview_kind_token(kind: PreviewKind) -> &'static str {
    match kind {
        PreviewKind::Tiff => "tiff",
        PreviewKind::Wmf => "wmf",
    }
}
