//! Per-type compose: Markdown.
//!
//! Pushes the syntax-highlighted source view. The rendered view lands in
//! a follow-up commit once the markdown renderer is wired in.

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{Detected, FileType};
use crate::viewer::ComposeCtx;
use crate::viewer::modes::Mode;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    modes.push(ctx.text_content_mode(source, &FileType::Markdown, args)?);
    Ok(())
}
