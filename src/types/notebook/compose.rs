//! Per-type compose: Jupyter notebook — rendered cell view + raw JSON
//! source.
//!
//! Mirrors Markdown: rendered-first by default (the cell view is the
//! point), `--raw` swaps the JSON source to the entry view (rendered
//! still reachable via Tab), `--plain` drops the rendered view entirely.
//! The source view routes through the generic structured-JSON content
//! mode, so it pretty-prints the notebook JSON and `r` toggles raw.

use std::rc::Rc;

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{Detected, FileType, StructuredFormat};
use crate::types::notebook::{NotebookRenderer, listing};
use crate::viewer::ComposeCtx;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::{Mode, RenderedTextMode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let rendered = (!ctx.plain_mode).then(|| -> Box<dyn Mode> {
        Box::new(RenderedTextMode::new(NotebookRenderer::new(
            source.clone(),
            Rc::clone(&ctx.theme_manager),
        )))
    });
    let source_mode =
        ctx.text_content_mode(source, &FileType::Structured(StructuredFormat::Json), args)?;

    match (rendered, args.raw) {
        (Some(r), false) => {
            modes.push(r);
            modes.push(source_mode);
        }
        (Some(r), true) => {
            modes.push(source_mode);
            modes.push(r);
        }
        (None, _) => {
            modes.push(source_mode);
        }
    }

    // Blocks TOC: code cells + image outputs as an extractable / descendable
    // listing. Skipped when the notebook has none (e.g. all-markdown).
    let entries = match source.read_text() {
        Ok(text) => listing::block_entries(&text),
        Err(_) => Vec::new(),
    };
    if !entries.is_empty() {
        modes.push(Box::new(ListingMode::new(
            "ipynb",
            "Blocks",
            entries,
            Vec::new(),
        )));
    }
    Ok(())
}
