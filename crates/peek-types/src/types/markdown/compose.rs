//! Per-type compose: Markdown — rendered text view + syntax-highlighted
//! source.
//!
//! Default order is rendered-first (matches Html / EPUB / Pdf / DOCX —
//! the rich-document UX). `--raw` swaps the order so the syntax-
//! highlighted source becomes the entry view; rendered is still
//! reachable via Tab. `--plain` drops the rendered view entirely
//! (consistent with `--plain` meaning "no transformation").

use std::rc::Rc;

use anyhow::Result;
use peek_detect::{Detected, FileType};
use peek_io::InputSource;

use crate::types::markdown::MarkdownRenderer;
use crate::viewer::ComposeCtx;
use crate::viewer::ComposeOpts;
use crate::viewer::modes::{Mode, RenderedTextMode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &ComposeOpts,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let rendered = (!args.plain).then(|| -> Box<dyn Mode> {
        Box::new(RenderedTextMode::new(MarkdownRenderer::new(
            source.clone(),
            Rc::clone(&ctx.theme_manager),
        )))
    });
    let source_mode = ctx.text_content_mode(source, &FileType::Markdown, args, None)?;

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
    Ok(())
}
