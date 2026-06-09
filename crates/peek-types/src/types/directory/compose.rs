//! Per-type compose: filesystem directory — one-level listing view.

use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::types::directory::{DirListSource, read};
use crate::viewer::ComposeCtx;
use crate::viewer::ComposeOpts;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::Mode;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    _args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let path = source
        .disk_path()
        .expect("Directory FileType implies a path");
    let (entries, warnings) = match read::read_dir_entries(path) {
        Ok(e) => (e, Vec::new()),
        Err(e) => (Vec::new(), vec![format!("Failed to read directory: {e:#}")]),
    };
    let show_parent = parent_link_enabled(path);
    let source = DirListSource::new(entries, show_parent);
    modes.push(Box::new(ListingMode::from_source(
        Box::new(source),
        "Listing",
        warnings,
    )));
    Ok(())
}

/// True when the directory at `path` has a parent we can navigate to.
/// Suppresses the `..` row at filesystem root (and on canonicalize
/// failures, where `..` would otherwise resolve to a broken target).
fn parent_link_enabled(path: &std::path::Path) -> bool {
    std::fs::canonicalize(path)
        .ok()
        .and_then(|p| p.parent().map(|_| ()))
        .is_some()
}
