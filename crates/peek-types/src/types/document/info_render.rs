//! The shared document info section (DOCX / ODT / RTF), driven by one
//! [`DocumentView`] that derives both `serde::Serialize` (JSON) and
//! [`InfoView`](crate::info::InfoView) (themed print). [`DocumentStats`] stays
//! the gather struct; the view projects it. The section title is the format
//! name; metadata members render inline (so they flatten into the JSON object
//! too, rather than nesting under a separate `metadata` key).

use serde::{Serialize, Serializer};

use crate::info::{Muted, Value};
use peek_detect::DocumentFormat;

use super::info::DocumentStats;

crate::info_section!(DocumentStats, DocumentView, "document");

#[derive(Serialize, crate::info::InfoView)]
#[info(title_from = "section_title")]
struct DocumentView {
    // The format token drives the JSON `format` field and the section title;
    // it is never a row of its own.
    #[info(skip)]
    #[serde(rename = "format", serialize_with = "ser_format")]
    format: DocumentFormat,
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
    #[info(label = "Paragraphs", skip_if_zero)]
    paragraph_count: Value,
    #[info(label = "Words", skip_if_zero)]
    word_count: Value,
    #[info(label = "Images", skip_if_zero)]
    image_count: Value,
    #[info(label = "Description")]
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<Muted>,
}

impl DocumentView {
    /// Section title — the uppercase format name.
    fn section_title(&self) -> &'static str {
        match self.format {
            DocumentFormat::Docx => "DOCX",
            DocumentFormat::Odt => "ODT",
            DocumentFormat::Rtf => "RTF",
        }
    }
}

impl From<&DocumentStats> for DocumentView {
    fn from(s: &DocumentStats) -> Self {
        let m = &s.metadata;
        DocumentView {
            format: s.format,
            title: m.title.clone(),
            creator: m.creator.clone(),
            subject: m.subject.clone().map(Muted),
            keywords: m.keywords.clone().map(Muted),
            created: m.created.map(Value::timestamp),
            modified: m.modified.map(Value::timestamp),
            paragraph_count: Value::count(s.paragraph_count as u64),
            word_count: Value::count(s.word_count as u64),
            image_count: Value::count(s.image_count as u64),
            description: m.description.clone().map(Muted),
        }
    }
}

fn ser_format<S: Serializer>(format: &DocumentFormat, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(match format {
        DocumentFormat::Docx => "docx",
        DocumentFormat::Odt => "odt",
        DocumentFormat::Rtf => "rtf",
    })
}
