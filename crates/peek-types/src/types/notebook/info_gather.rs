//! Build [`NotebookInfo`] from a notebook source for the Info section.

use crate::info::Extras;
use peek_io::InputSource;

use super::info::NotebookInfo;
use super::model::Notebook;

/// Parse the notebook and collect its info sidecar. Whole-file read:
/// notebook structure needs a full JSON parse, and the same carve-out
/// that justifies structured pretty-print applies here. Returns `None`
/// when the bytes don't parse as a notebook, so the gather falls back to
/// the generic binary/text path.
pub fn gather_extras(source: &InputSource) -> Option<Extras> {
    let text = source
        .read_text(peek_io::limits::Budget::WholeDoc("notebook"))
        .ok()?;
    let nb = Notebook::parse(&text)?;
    Some(Box::new(NotebookInfo::from_notebook(&nb)))
}
