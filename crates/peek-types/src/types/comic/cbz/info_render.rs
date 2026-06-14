//! The comic-archive info section, driven by one [`ComicView`] that derives
//! both `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView)
//! (themed print). [`ComicStats`] stays the gather struct; the view projects
//! it.

use serde::{Serialize, Serializer};
use serde_json::json;

use crate::info::{Role, Value, render_info, thousands_sep};
use crate::types::comic::ComicStats;
use peek_detect::ComicFormat;
use peek_theme::PeekTheme;

/// Themed terminal comic section.
pub fn render_section(lines: &mut Vec<String>, stats: &ComicStats, theme: &PeekTheme) {
    render_info(lines, &ComicView::from(stats), theme);
}

/// Typed `--info --json` view of the comic section, nested under `"comic"`.
pub fn json_section(stats: &ComicStats) -> (&'static str, serde_json::Value) {
    (
        "comic",
        serde_json::to_value(ComicView::from(stats)).expect("comic info view serializes"),
    )
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title_from = "section_title")]
struct ComicView {
    #[info(skip)]
    #[serde(rename = "format", serialize_with = "ser_format")]
    format: ComicFormat,
    #[info(label = "Pages", skip_if_zero)]
    page_count: Value,
    #[info(label = "Image bytes", skip_if_zero)]
    total_image_bytes: Value,
}

impl ComicView {
    fn section_title(&self) -> &'static str {
        self.format.label()
    }
}

impl From<&ComicStats> for ComicView {
    fn from(s: &ComicStats) -> Self {
        ComicView {
            format: s.format,
            page_count: Value::count(s.page_count as u64),
            total_image_bytes: Value::split(
                format!("{} bytes", thousands_sep(s.total_image_bytes)),
                Role::Muted,
                json!(s.total_image_bytes),
            ),
        }
    }
}

fn ser_format<S: Serializer>(format: &ComicFormat, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(match format {
        ComicFormat::Cbz => "cbz",
    })
}
