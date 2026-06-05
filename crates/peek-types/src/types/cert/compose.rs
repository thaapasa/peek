//! Per-type compose for cert / key files. The source view depends on the
//! container: PEM shows its plain text; JWK shows the pretty-printed JSON
//! (via the shared structured content mode); raw DER is binary and has no
//! text source — Info + the universal hex tail carry it. The rich decode
//! lives in the Info aux mode (appended by `Registry::compose_modes`,
//! populated from the cert `CertInfo` extras) for every container.

use std::rc::Rc;

use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::{CertFormat, Detected, FileType, StructuredFormat};
use crate::viewer::ComposeCtx;
use crate::viewer::ComposeOpts;
use crate::viewer::modes::{ContentMode, ContentModeConfig, Mode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &ComposeOpts,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: CertFormat,
) -> Result<()> {
    match fmt {
        // DER is binary: no source view, just Info + the universal hex tail.
        CertFormat::Der => {}
        // JWK is JSON: reuse the structured content mode so the source view
        // pretty-prints + highlights exactly like a standalone `.json`.
        CertFormat::Jwk => {
            let ft = FileType::Structured(StructuredFormat::Json);
            let pretty = crate::types::structured::pretty_view_for(&ft, ctx.plain_mode);
            modes.push(ctx.text_content_mode(source, &ft, args, pretty)?);
        }
        CertFormat::Pem => {
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
        }
    }
    Ok(())
}
