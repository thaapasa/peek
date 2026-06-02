//! Per-type compose for cert / key files. PEM gets the source text
//! viewer; raw DER is binary, so it has no text source — its only view
//! is the universal Info aux mode (appended by `Registry::compose_modes`
//! and populated from `FileExtras::Cert`) plus the hex dump. The rich
//! decode lives in the Info section either way.

use std::rc::Rc;

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{CertFormat, Detected};
use crate::viewer::ComposeCtx;
use crate::viewer::modes::{ContentMode, ContentModeConfig, Mode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: CertFormat,
) -> Result<()> {
    // DER is binary: no source view, just Info + the universal hex tail.
    if fmt == CertFormat::Der {
        return Ok(());
    }
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
