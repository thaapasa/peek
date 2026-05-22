//! Per-type compose: DOCX / ODT (shared AST + ZIP TOC) and RTF
//! (painter-tagged stream + inline-embed TOC).

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{ArchiveFormat, Detected, DocumentFormat};
use crate::types::archive;
use crate::types::document::{self, DocRenderer, rtf::RtfRenderer};
use crate::viewer::ComposeCtx;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::{Mode, RenderedTextMode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    _args: &Args,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: DocumentFormat,
) -> Result<()> {
    // Per-format dispatch lives here, the one match — `compose_zip`
    // takes the already-parsed doc so it never re-matches `fmt`.
    match fmt {
        DocumentFormat::Docx => compose_zip(
            source,
            document::docx::package::open(source),
            fmt.label(),
            modes,
        ),
        DocumentFormat::Odt => compose_zip(
            source,
            document::odt::package::open(source),
            fmt.label(),
            modes,
        ),
        DocumentFormat::Rtf => compose_rtf(source, modes),
    }
}

/// Wire a ZIP-backed word document into the read view + ZIP TOC.
/// `parsed` is the format's already-attempted `ast::Doc`; `label`
/// names it in warnings and the TOC. Format-agnostic — DOCX and ODT
/// both parse to `ast::Doc` and feed this same pipeline.
fn compose_zip(
    source: &InputSource,
    parsed: Result<document::ast::Doc>,
    label: &'static str,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let mut warnings = Vec::new();
    match parsed {
        Ok(doc) => modes.push(Box::new(RenderedTextMode::new(DocRenderer::new(doc)))),
        Err(e) => warnings.push(format!("{label} unreadable: {e:#}")),
    }
    let (entries, mut listing_warnings) =
        match archive::reader::list_entries(source, ArchiveFormat::Zip) {
            Ok(e) => (e, Vec::new()),
            Err(e) => (Vec::new(), vec![format!("Failed to list {label}: {e:#}")]),
        };
    warnings.append(&mut listing_warnings);
    modes.push(Box::new(ListingMode::new(label, "TOC", entries, warnings)));
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
        modes.push(Box::new(RenderedTextMode::new(RtfRenderer::new(parsed))));
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
