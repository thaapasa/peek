//! Per-type compose: DOCX / ODT (shared AST + ZIP TOC) and RTF
//! (painter-tagged stream + inline-embed TOC).

use anyhow::Result;
use peek_detect::{Detected, DocumentFormat};
use peek_io::InputSource;

use crate::types::archive;
use crate::types::document::rtf::RtfRenderer;
use crate::types::document::{self, DocRenderer};
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::{Mode, RenderedTextMode};
use crate::viewer::{ComposeCtx, ComposeOpts};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    _args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: DocumentFormat,
) -> Result<()> {
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
    archive::reader::push_zip_toc(source, label, warnings, modes);
    Ok(())
}

fn compose_rtf(source: &InputSource, modes: &mut Vec<Box<dyn Mode>>) -> Result<()> {
    // RTF is single-file at the container level, but real Word RTFs
    // embed images as `\pict` groups inline with the prose. Read view
    // + a synthetic listing of those embeds keeps the same TAB
    // workflow as ZIP-backed DOCX.
    match document::rtf::parse::open_source(source) {
        Ok(parsed) => {
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
        // No read mode (over the render cap, or unparseable). Carry the
        // reason on a TOC listing so it surfaces through Info; the hex
        // view stands in for the raw bytes.
        Err(e) => modes.push(Box::new(ListingMode::new(
            DocumentFormat::Rtf.label(),
            "TOC",
            Vec::new(),
            vec![format!("RTF unreadable: {e:#}")],
        ))),
    }
    Ok(())
}
