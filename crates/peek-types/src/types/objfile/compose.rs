//! Per-type compose: object files. InfoMode is the landing view (the
//! header summary — mirrors `file` / `readelf -h`); a Sections table and a
//! Symbols listing follow in the Tab cycle. Selecting a symbol jumps the
//! Hex view to its byte offset; there is no extract path.

use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::viewer::ComposeCtx;
use crate::viewer::ComposeOpts;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::{InfoMode, Mode};
use crate::viewer::table::TableMode;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    _args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    // Info first — the most useful single view for an object file. The
    // universal tail dedupes by ModeId, so the later Info append is a
    // no-op.
    modes.push(Box::new(InfoMode::new()));

    // Parse once: the Sections table and the Symbols listing share the
    // single `object::File` view. A parse failure leaves the stack
    // Info-only; the Info section surfaces the same error.
    if let Ok(bytes) = source.read_bytes(crate::input::limits::Budget::BulkWalk("object file"))
        && let Ok(loaded) = super::load::load(&bytes)
    {
        let sections = super::tables::build_sections(&loaded.file);
        modes.push(Box::new(TableMode::new("Sections", sections)));
        let (symbols, warnings) =
            super::symbol_list::build_from_file(&loaded.file, loaded.slice_offset);
        modes.push(Box::new(ListingMode::from_source(
            Box::new(symbols),
            "Symbols",
            warnings,
        )));
    }
    Ok(())
}
