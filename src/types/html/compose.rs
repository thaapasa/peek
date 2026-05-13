//! Per-type compose: HTML — rendered text view + raw HTML source.

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{Detected, FileType};
use crate::types::html::RenderedMode;
use crate::viewer::ComposeCtx;
use crate::viewer::modes::Mode;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    modes.push(Box::new(RenderedMode::new(
        source.clone(),
        ctx.peek_theme.style_mode,
    )));
    modes.push(ctx.text_content_mode(source, &FileType::Html, args)?);
    Ok(())
}
