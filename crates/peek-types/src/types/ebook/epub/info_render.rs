//! The EPUB info section, driven by one [`EbookView`] that derives both
//! `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView) (themed
//! print). [`EbookStats`] stays the gather struct; the view projects it.
//! Metadata members render inline (and so flatten into the JSON object).

use crate::info::{Muted, Value, render_info};
use crate::types::ebook::EbookStats;
use peek_theme::PeekTheme;

/// Themed terminal EPUB section.
pub fn render_section(lines: &mut Vec<String>, stats: &EbookStats, theme: &PeekTheme) {
    render_info(lines, &EbookView::from(stats), theme);
}

/// Typed `--info --json` view of the EPUB section, nested under `"ebook"`.
pub fn json_section(stats: &EbookStats) -> (&'static str, serde_json::Value) {
    (
        "ebook",
        serde_json::to_value(EbookView::from(stats)).expect("ebook info view serializes"),
    )
}

#[derive(serde::Serialize, crate::info::InfoView)]
#[info(title = "EPUB")]
struct EbookView {
    #[info(label = "Title")]
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[info(label = "Author")]
    #[serde(skip_serializing_if = "Option::is_none")]
    creator: Option<String>,
    #[info(label = "Language")]
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<Muted>,
    #[info(label = "Publisher")]
    #[serde(skip_serializing_if = "Option::is_none")]
    publisher: Option<Muted>,
    #[info(label = "Date")]
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<Muted>,
    #[info(label = "Identifier")]
    #[serde(skip_serializing_if = "Option::is_none")]
    identifier: Option<Muted>,
    // JSON keeps the count; print hides a zero.
    #[info(label = "Chapters", skip_if_zero)]
    chapter_count: Value,
    #[info(label = "Description")]
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<Muted>,
}

impl From<&EbookStats> for EbookView {
    fn from(s: &EbookStats) -> Self {
        let m = &s.metadata;
        EbookView {
            title: m.title.clone(),
            creator: m.creator.clone(),
            language: m.language.clone().map(Muted),
            publisher: m.publisher.clone().map(Muted),
            date: m.date.clone().map(Muted),
            identifier: m.identifier.clone().map(Muted),
            chapter_count: Value::count(s.chapter_count as u64),
            description: m.description.clone().map(Muted),
        }
    }
}
