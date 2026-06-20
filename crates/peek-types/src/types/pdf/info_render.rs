//! The PDF info section, driven by one [`PdfView`] that derives both
//! `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView) (themed
//! print). Mirrors the shared document layout. [`PdfStats`] stays the gather
//! struct; the view projects it.
//!
//! On a load error only the `Error` row shows: the gather leaves every other
//! field empty/zero, so they skip. Zero counts are omitted from *both* outputs
//! (modelled as `Option<Value>`); `encrypted` is a JSON bool always but a
//! print row only when set.

use serde::{Serialize, Serializer};

use crate::info::{InfoValue, Muted, Value, Warn};
use crate::types::pdf::PdfFlavor;
use peek_theme::PeekTheme;

use super::info::PdfStats;

crate::info_section!(PdfStats, PdfView, "pdf");

#[derive(Serialize, crate::info::InfoView)]
#[info(title_from = "section_title")]
struct PdfView {
    #[info(skip)]
    #[serde(rename = "flavor", serialize_with = "ser_flavor")]
    flavor: PdfFlavor,
    #[info(label = "Error")]
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Warn>,
    #[info(label = "Version")]
    #[serde(skip_serializing_if = "String::is_empty")]
    pdf_version: String,
    // JSON bool always; print row only when encrypted.
    #[info(label = "Encrypted", skip_if = "Encrypted::clear")]
    encrypted: Encrypted,
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
    // Zero counts vanish from both outputs.
    #[info(label = "Pages")]
    #[serde(skip_serializing_if = "Option::is_none")]
    page_count: Option<Value>,
    #[info(label = "Attachments")]
    #[serde(skip_serializing_if = "Option::is_none")]
    attachment_count: Option<Value>,
    #[info(label = "Images")]
    #[serde(skip_serializing_if = "Option::is_none")]
    image_count: Option<Value>,
    #[info(label = "Description")]
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<Muted>,
}

impl PdfView {
    fn section_title(&self) -> &'static str {
        self.flavor.label()
    }
}

impl From<&PdfStats> for PdfView {
    fn from(s: &PdfStats) -> Self {
        let m = &s.metadata;
        let count = |n: usize| (n > 0).then(|| Value::count(n as u64));
        PdfView {
            flavor: s.flavor,
            error: s.error.clone().map(Warn),
            pdf_version: s.pdf_version.clone(),
            encrypted: Encrypted(s.encrypted),
            title: m.title.clone(),
            creator: m.creator.clone(),
            subject: m.subject.clone().map(Muted),
            keywords: m.keywords.clone().map(Muted),
            created: m.created.map(Value::timestamp),
            modified: m.modified.map(Value::timestamp),
            page_count: count(s.page_count),
            attachment_count: count(s.attachment_count),
            image_count: count(s.image_count),
            description: m.description.clone().map(Muted),
        }
    }
}

/// `encrypted` flag: a JSON bool, but printed only when set — as a warning
/// `yes` (an encrypted document is the noteworthy case).
struct Encrypted(bool);

impl Encrypted {
    /// Skip predicate: hide the print row when not encrypted.
    fn clear(&self) -> bool {
        !self.0
    }
}

impl InfoValue for Encrypted {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_warning("yes")
    }
}

impl Serialize for Encrypted {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_bool(self.0)
    }
}

fn ser_flavor<S: Serializer>(flavor: &PdfFlavor, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(match flavor {
        PdfFlavor::Pdf => "pdf",
        PdfFlavor::Illustrator => "illustrator",
    })
}
