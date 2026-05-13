//! Per-type compose: DOCX / ODT (shared AST + ZIP TOC) and RTF
//! (painter-tagged stream + inline-embed TOC).

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{ArchiveFormat, Detected, DocumentFormat};
use crate::types::archive;
use crate::types::document::{self, DocReadMode, rtf::RtfReadMode};
use crate::viewer::ComposeCtx;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::Mode;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    _args: &Args,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: DocumentFormat,
) -> Result<()> {
    match fmt {
        DocumentFormat::Docx | DocumentFormat::Odt => compose_zip(source, fmt, modes),
        DocumentFormat::Rtf => compose_rtf(source, modes),
    }
}

fn compose_zip(
    source: &InputSource,
    fmt: DocumentFormat,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let mut warnings = Vec::new();
    let parsed = match fmt {
        DocumentFormat::Docx => document::docx::package::open(source),
        DocumentFormat::Odt => document::odt::package::open(source),
        DocumentFormat::Rtf => unreachable!("RTF handled by compose_rtf"),
    };
    match parsed {
        Ok(doc) => modes.push(Box::new(DocReadMode::new(source.clone(), doc))),
        Err(e) => warnings.push(format!("{} unreadable: {e:#}", fmt.label())),
    }
    let (entries, mut listing_warnings) =
        match archive::reader::list_entries(source, ArchiveFormat::Zip) {
            Ok(e) => (e, Vec::new()),
            Err(e) => (
                Vec::new(),
                vec![format!("Failed to list {}: {e:#}", fmt.label())],
            ),
        };
    warnings.append(&mut listing_warnings);
    modes.push(Box::new(ListingMode::new(
        fmt.label(),
        "TOC",
        entries,
        warnings,
    )));
    Ok(())
}

fn compose_rtf(source: &InputSource, modes: &mut Vec<Box<dyn Mode>>) -> Result<()> {
    // RTF is single-file at the container level, but real Word RTFs
    // embed images as `\pict` groups inline with the prose. Read view
    // + a synthetic listing of those embeds keeps the same TAB
    // workflow as ZIP-backed DOCX.
    if let Ok(parsed) = document::rtf::parse::open_source(source) {
        let entries = document::rtf::parse::embeds_to_entries(&parsed.embeds);
        let has_embeds = !entries.is_empty();
        modes.push(Box::new(RtfReadMode::new(source.clone(), parsed)));
        if has_embeds {
            modes.push(Box::new(ListingMode::new(
                DocumentFormat::Rtf.label(),
                "TOC",
                entries,
                Vec::new(),
            )));
        }
    }
    // Parse error: surface through Info instead of pushing a read mode.
    Ok(())
}
