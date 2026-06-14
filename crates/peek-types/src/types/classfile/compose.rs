//! Per-type compose: Java classfiles. InfoMode is the landing view (the
//! header summary); Fields and Methods follow as table views the Tab
//! cycle steps through. No extract path.

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
    // Info first — the header summary is the natural landing view. The
    // universal tail dedupes by ModeId, so the later Info append is a
    // no-op.
    modes.push(Box::new(InfoMode::new()));

    // Two table views. A parse failure leaves the stack Info-only; the
    // Info section surfaces the same error.
    if let Ok(tables) = super::tables::build(source) {
        modes.push(Box::new(TableMode::new("Fields", tables.fields)));
        modes.push(Box::new(TableMode::new("Methods", tables.methods)));
    }

    // Bytecode disassembly — parsed separately (with bytecode enabled) so
    // a parse failure here doesn't sink the cheaper metadata views above.
    if let Ok(disasm) = super::bytecode::build(source) {
        modes.push(Box::new(super::bytecode_mode::BytecodeMode::new(disasm)));
    }
    Ok(())
}
