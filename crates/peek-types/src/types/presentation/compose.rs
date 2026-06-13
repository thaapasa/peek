//! Per-type compose: PPTX / ODP (per-slide text reader + ZIP TOC) and
//! Keynote (embedded-preview image view + ZIP TOC).

use std::sync::Arc;

use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::{ArchiveFormat, Detected};
use crate::types::archive;
use crate::types::presentation::keynote::preview::PreviewRenderer;
use crate::types::presentation::read_mode::PresentationReadMode;
use crate::types::presentation::{self, Deck, PresentationFormat};
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::Mode;
use crate::viewer::paged::PagedImageMode;
use crate::viewer::{ComposeCtx, ComposeOpts, image_config};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: PresentationFormat,
) -> Result<()> {
    let label = fmt.label();
    match fmt {
        PresentationFormat::Pptx | PresentationFormat::Pptm | PresentationFormat::Ppsx => {
            compose_deck(
                source,
                presentation::pptx::package::open(source),
                label,
                modes,
            )
        }
        PresentationFormat::Odp => compose_deck(
            source,
            presentation::odp::package::open(source),
            label,
            modes,
        ),
        PresentationFormat::Key => compose_keynote(source, args, modes),
    }
}

/// Wire a parsed slide deck into the slide read view + ZIP TOC. PPTX and
/// ODP both parse to a [`Deck`] and feed this same pipeline.
fn compose_deck(
    source: &InputSource,
    parsed: Result<Deck>,
    label: &'static str,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let mut warnings = Vec::new();
    match parsed {
        Ok(deck) if !deck.slides.is_empty() => {
            modes.push(Box::new(PresentationReadMode::new(deck.slides)))
        }
        Ok(_) => warnings.push(format!("{label} has no readable slides")),
        Err(e) => warnings.push(format!("{label} unreadable: {e:#}")),
    }
    push_zip_toc(source, label, warnings, modes);
    Ok(())
}

/// Keynote: render the embedded preview thumbnail as the primary view,
/// plus the raw ZIP TOC. No slide-text extraction (IWA is undocumented
/// protobuf).
fn compose_keynote(
    source: &InputSource,
    args: &ComposeOpts,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let mut warnings = Vec::new();
    match presentation::keynote::package::open(source) {
        Ok(kn) => match kn.preview {
            Some(bytes) => match image::load_from_memory(&bytes) {
                Ok(img) => modes.push(Box::new(PagedImageMode::with_label(
                    PreviewRenderer::new(Arc::new(img)),
                    image_config(args),
                    "Preview",
                ))),
                Err(e) => warnings.push(format!("Keynote preview undecodable: {e:#}")),
            },
            None => warnings.push("Keynote has no embedded preview".to_string()),
        },
        Err(e) => warnings.push(format!("Keynote unreadable: {e:#}")),
    }
    push_zip_toc(source, "Keynote", warnings, modes);
    Ok(())
}

fn push_zip_toc(
    source: &InputSource,
    label: &'static str,
    mut warnings: Vec<String>,
    modes: &mut Vec<Box<dyn Mode>>,
) {
    let (entries, mut listing_warnings) =
        match archive::reader::list_entries(source, ArchiveFormat::Zip) {
            Ok((e, _)) => (e, Vec::new()),
            Err(e) => (Vec::new(), vec![format!("Failed to list {label}: {e:#}")]),
        };
    warnings.append(&mut listing_warnings);
    modes.push(Box::new(ListingMode::new(label, "TOC", entries, warnings)));
}
