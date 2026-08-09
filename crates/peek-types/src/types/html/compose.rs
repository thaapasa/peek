//! Per-type compose: HTML — rendered text view + raw HTML source.

use anyhow::Result;
use peek_detect::{Detected, FileType};
use peek_io::InputSource;

use crate::types::html::HtmlRenderer;
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
    // `--plain` drops the html2text render — HTML falls back to raw
    // source, consistent with `--plain` meaning "no transformation"
    // for every other text type.
    if !args.plain {
        modes.push(Box::new(RenderedTextMode::new(HtmlRenderer::new(
            source.clone(),
        ))));
    }
    modes.push(ctx.text_content_mode(source, &FileType::Html, args, None)?);
    Ok(())
}
