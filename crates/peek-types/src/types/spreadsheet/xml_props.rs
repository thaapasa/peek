//! Core document properties from the workbook container.
//!
//! OOXML stores them in `docProps/core.xml`, ODS in `meta.xml`. Both
//! lean on Dublin Core (`dc:title`, `dc:creator`, …) with a few
//! vocabulary differences for keywords / dates, so one parser keyed on
//! the full prefixed element names covers both.

use peek_io::InputSource;
use quick_xml::Reader;
use quick_xml::events::Event;

use super::SpreadsheetFormat;
use crate::types::archive::reader::{open_zip, read_zip_entry_str};
use crate::types::document::DocumentMetadata;

/// Read + parse the container's metadata XML. `None` on any failure
/// (unreadable zip, missing entry) — metadata is best-effort.
pub(crate) fn read_metadata(
    source: &InputSource,
    fmt: SpreadsheetFormat,
) -> Option<DocumentMetadata> {
    let mut zip = open_zip(source, "workbook").ok()?;
    let entry = if fmt.is_ooxml() {
        "docProps/core.xml"
    } else {
        "meta.xml"
    };
    let xml = read_zip_entry_str(&mut zip, entry, "workbook").ok()?;
    Some(parse_props(&xml))
}

#[derive(Clone, Copy)]
enum Field {
    Title,
    Creator,
    Subject,
    Description,
    Keywords,
    Created,
    Modified,
}

/// Map a prefixed element name to the metadata field it feeds. Covers
/// both the OOXML core-properties vocabulary and the ODS `meta.xml` one.
fn field_for(name: &str) -> Option<Field> {
    match name {
        "dc:title" => Some(Field::Title),
        // OOXML uses `dc:creator`; ODS prefers `meta:initial-creator`
        // but also carries `dc:creator` — first non-empty wins.
        "dc:creator" | "meta:initial-creator" => Some(Field::Creator),
        "dc:subject" => Some(Field::Subject),
        "dc:description" => Some(Field::Description),
        "cp:keywords" | "meta:keyword" => Some(Field::Keywords),
        "dcterms:created" | "meta:creation-date" => Some(Field::Created),
        // OOXML modified = `dcterms:modified`; ODS = `dc:date`.
        "dcterms:modified" | "dc:date" => Some(Field::Modified),
        _ => None,
    }
}

fn parse_props(xml: &str) -> DocumentMetadata {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut meta = DocumentMetadata::default();
    let mut current: Option<Field> = None;
    let mut text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                current = field_for(e.name().as_ref());
                text.clear();
            }
            Ok(Event::Text(t)) if current.is_some() => {
                text.push_str(&t.xml10_content());
            }
            Ok(Event::End(_)) => {
                if let Some(field) = current.take() {
                    let value = text.trim();
                    if !value.is_empty() {
                        assign(&mut meta, field, value);
                    }
                    text.clear();
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    meta
}

/// First non-empty value wins (handles ODS carrying both
/// `meta:initial-creator` and `dc:creator`).
fn assign(meta: &mut DocumentMetadata, field: Field, value: &str) {
    let slot = match field {
        Field::Title => &mut meta.title,
        Field::Creator => &mut meta.creator,
        Field::Subject => &mut meta.subject,
        Field::Description => &mut meta.description,
        Field::Keywords => &mut meta.keywords,
        // ISO-8601 dates (`dcterms:created` / `dc:date` / …) — parse to instants.
        Field::Created => return meta.set_created_iso(value),
        Field::Modified => return meta.set_modified_iso(value),
    };
    if slot.is_none() {
        *slot = Some(value.to_string());
    }
}
