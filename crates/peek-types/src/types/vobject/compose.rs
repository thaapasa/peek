//! Per-type compose for vObject documents.
//!
//! Both flavours get the same two-view stack: a rendered read view (the
//! pretty agenda / contact-card list) followed by the raw source via the
//! generic content mode. `--plain` drops the rendered view so the raw
//! text is the first (print/pipe) mode.

use anyhow::Result;
use peek_detect::{Detected, FileType, VObjectFormat};
use peek_io::InputSource;

use super::calendar::CalendarRenderer;
use super::contact::ContactRenderer;
use crate::viewer::modes::{Mode, RenderedTextMode};
use crate::viewer::{ComposeCtx, ComposeOpts};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &ComposeOpts,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: VObjectFormat,
) -> Result<()> {
    if !args.plain {
        modes.push(match fmt {
            VObjectFormat::ICal => {
                Box::new(RenderedTextMode::new(CalendarRenderer::new(source.clone())))
            }
            VObjectFormat::VCard => {
                Box::new(RenderedTextMode::new(ContactRenderer::new(source.clone())))
            }
        });
    }
    modes.push(ctx.text_content_mode(source, &FileType::VObject(fmt), args, None)?);
    Ok(())
}
