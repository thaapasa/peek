//! Per-type compose: Apple `.DS_Store`. The Records table is the landing
//! view — the records *are* the content here (like CSV / SQLite) — with
//! the Info summary next in the Tab cycle. No source view (opaque binary
//! — the universal Hex tail covers raw bytes) and no extract path (the
//! records aren't files).

use anyhow::Result;

use crate::viewer::ComposeCtx;
use crate::viewer::ComposeOpts;
use crate::viewer::modes::{InfoMode, Mode};
use crate::viewer::table::TableMode;
use peek_detect::Detected;
use peek_io::InputSource;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    _args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    // Records table first — the landing view, and what the print/pipe path
    // emits. A parse failure pushes nothing here, so Info (appended below)
    // becomes the landing view and surfaces the same error.
    if let Ok(bytes) = source.read_bytes(peek_io::limits::Budget::Sidecar(".DS_Store"))
        && let Ok(store) = super::reader::parse(&bytes)
    {
        modes.push(Box::new(TableMode::new(
            "Records",
            super::tables::build(&store),
        )));
    }

    // Info next in the cycle. The universal tail also appends Info and
    // dedupes by ModeId, so this explicit push only fixes the ordering.
    modes.push(Box::new(InfoMode::new()));
    Ok(())
}
