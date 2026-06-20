//! The shared presentation info section (PPTX / ODP / Keynote), driven
//! by one [`PresentationView`] that derives both `serde::Serialize`
//! (JSON) and [`InfoView`](crate::info::InfoView) (themed print).
//! [`PresentationStats`] stays the gather struct; the view projects it.
//! The section title is the format name; metadata members render inline
//! (so they flatten into the JSON object rather than nesting).

use serde::{Serialize, Serializer};

use crate::info::{Muted, Value};
use peek_detect::PresentationFormat;

use super::info::PresentationStats;

crate::info_section!(PresentationStats, PresentationView, "presentation");

#[derive(Serialize, crate::info::InfoView)]
#[info(title_from = "section_title")]
struct PresentationView {
    // Drives the JSON `format` field and the section title; never a row.
    #[info(skip)]
    #[serde(rename = "format", serialize_with = "ser_format")]
    format: PresentationFormat,
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
    #[info(label = "Application")]
    #[serde(skip_serializing_if = "Option::is_none")]
    application: Option<Muted>,
    #[info(label = "Created")]
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<Value>,
    #[info(label = "Modified")]
    #[serde(skip_serializing_if = "Option::is_none")]
    modified: Option<Value>,
    #[info(label = "Slides", skip_if_zero)]
    slide_count: Value,
    #[info(label = "Words", skip_if_zero)]
    word_count: Value,
    #[info(label = "Images", skip_if_zero)]
    image_count: Value,
}

impl PresentationView {
    /// Section title — the uppercase format name.
    fn section_title(&self) -> &'static str {
        match self.format {
            PresentationFormat::Pptx => "PPTX",
            PresentationFormat::Pptm => "PPTM",
            PresentationFormat::Ppsx => "PPSX",
            PresentationFormat::Odp => "ODP",
            PresentationFormat::Key => "Keynote",
        }
    }
}

impl From<&PresentationStats> for PresentationView {
    fn from(s: &PresentationStats) -> Self {
        let m = &s.metadata;
        PresentationView {
            format: s.format,
            title: m.title.clone(),
            creator: m.creator.clone(),
            subject: m.subject.clone().map(Muted),
            keywords: m.keywords.clone().map(Muted),
            application: m.application.clone().map(Muted),
            created: m.created.map(Value::timestamp),
            modified: m.modified.map(Value::timestamp),
            slide_count: Value::count(s.slide_count as u64),
            word_count: Value::count(s.word_count as u64),
            image_count: Value::count(s.image_count as u64),
        }
    }
}

fn ser_format<S: Serializer>(format: &PresentationFormat, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(match format {
        PresentationFormat::Pptx => "pptx",
        PresentationFormat::Pptm => "pptm",
        PresentationFormat::Ppsx => "ppsx",
        PresentationFormat::Odp => "odp",
        PresentationFormat::Key => "key",
    })
}
