//! Per-type compose for PEM / cert files. One mode: the source text
//! viewer. The rich Info section comes from the universal Info aux
//! mode appended by `Registry::compose_modes`, populated from
//! `FileExtras::Cert`.

use std::rc::Rc;

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::viewer::ComposeCtx;
use crate::viewer::modes::{ContentMode, ContentModeConfig, Mode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let line_source = source.open_line_source()?;
    modes.push(Box::new(ContentMode::new(
        source.clone(),
        line_source,
        Rc::clone(&ctx.theme_manager),
        ctx.theme_name,
        ContentModeConfig {
            label: "Source",
            line_numbers: args.line_numbers,
            ..Default::default()
        },
    )));
    Ok(())
}
