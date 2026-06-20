//! The EPUB info section, driven by one [`EbookView`] that derives both
//! `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView) (themed
//! print). [`EbookStats`] stays the gather struct; the view projects it.
//! Metadata members render inline (and so flatten into the JSON object).

use crate::info::{Muted, Value};
use crate::types::ebook::EbookStats;

crate::info_section!(EbookStats, EbookView, "ebook");

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
