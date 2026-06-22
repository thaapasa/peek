//! Keynote package open: locate the embedded preview thumbnail and the
//! cheap XML metadata.
//!
//! A flat `.key` is a ZIP carrying a QuickLook preview (`preview.jpg` >
//! `preview-web.jpg` > `preview-micro.jpg`, best first) plus a
//! `Metadata/BuildVersionHistory.plist` (an XML plist listing the build
//! versions that saved the file). `Metadata/Properties.plist` is a
//! *binary* plist and is skipped — reading it would pull in a plist
//! dependency for marginal gain.

use anyhow::{Result, bail};
use bytes::Bytes;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::types::archive::reader::{open_zip, read_zip_entry};
use crate::types::presentation::PresentationMetadata;
use peek_io::InputSource;

/// Preview entries in descending quality order.
const PREVIEW_CANDIDATES: &[&str] = &["preview.jpg", "preview-web.jpg", "preview-micro.jpg"];

pub struct Keynote {
    pub metadata: PresentationMetadata,
    /// Best available embedded preview bitmap, undecoded. `None` when the
    /// package carries no preview.
    pub preview: Option<Bytes>,
}

pub fn open(source: &InputSource) -> Result<Keynote> {
    let mut zip = open_zip(source, "Keynote")?;

    let preview = PREVIEW_CANDIDATES
        .iter()
        .find_map(|name| read_zip_entry(&mut zip, name, "Keynote").ok());

    let mut metadata = PresentationMetadata::default();
    if let Ok(xml) = read_zip_entry(&mut zip, "Metadata/BuildVersionHistory.plist", "Keynote")
        && let Ok(xml) = std::str::from_utf8(&xml)
    {
        metadata.application = build_version(xml).map(|v| format!("Keynote (build {v})"));
    }

    if preview.is_none() && metadata.application.is_none() {
        // Neither a preview nor recognisable metadata — not a shape we
        // can present beyond the raw ZIP listing.
        bail!("no Keynote preview or metadata found");
    }
    Ok(Keynote { metadata, preview })
}

/// Last `<string>` in the XML build-history plist — the build that most
/// recently saved the document (e.g. `M14.2-7038.0.74-2`).
fn build_version(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut last: Option<String> = None;
    let mut capturing = false;
    let mut text = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"string" => {
                capturing = true;
                text.clear();
            }
            Ok(Event::Text(t)) if capturing => {
                if let Ok(decoded) = t.xml10_content() {
                    text.push_str(&decoded);
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"string" => {
                capturing = false;
                let v = text.trim().to_string();
                if !v.is_empty() {
                    last = Some(v);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    last
}
