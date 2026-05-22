//! Per-type compose: HTML — rendered text view + raw HTML source.

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{Detected, FileType};
use crate::types::html::HtmlRenderer;
use crate::viewer::ComposeCtx;
use crate::viewer::modes::{Mode, RenderedTextMode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    modes.push(Box::new(RenderedTextMode::new(HtmlRenderer::new(
        source.clone(),
    ))));
    modes.push(ctx.text_content_mode(source, &FileType::Html, args)?);
    Ok(())
}
