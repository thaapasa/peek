//! `ListSource`: the per-consumer data + render + select provider behind
//! the generic listing engine ([`super::mode::ListingMode`]). The engine
//! owns navigation (scroll / selection / paging / sticky breadcrumb /
//! search); the source owns row data, the surrounding column painting, and
//! the extract key for a row. Selecting the one "name" column is the
//! engine's job — it overlays search matches and the selection highlight —
//! so a source only describes that column, it doesn't paint it.
//!
//! Today's sources: [`super::tree_source::TreeListSource`] (file-tree TOCs
//! — archives, ISO images, embedded-file lists, zip-backed documents), the
//! directory listing, email attachment lists, and object-file symbol
//! listings (jump-to-hex via `jump_target`). New sources plug in here
//! without touching the engine.

use crate::theme::PeekTheme;
use crate::viewer::modes::{ExtractTarget, ModeId, Position, RenderCtx};

use super::viewport::RowMeta;

/// One list view's rows, columns, and select semantics. Row indices are
/// stable for the source's lifetime; the engine caches navigation metadata
/// (`parent` / `selectable`) up front and queries the rest lazily per
/// render.
#[allow(clippy::len_without_is_empty)] // engine drives off row indices, never emptiness
pub trait ListSource {
    fn len(&self) -> usize;
    /// Parent row index for the sticky breadcrumb; `None` for a top-level
    /// row (so a flat listing leaves every row parentless).
    fn parent(&self, idx: usize) -> Option<usize>;
    /// Whether the selection can land on this row. Tree listings mark only
    /// files; flat listings mark every row.
    fn selectable(&self, idx: usize) -> bool;
    /// Leaf text the engine searches and shows in the sticky breadcrumb.
    fn name(&self, idx: usize) -> &str;
    /// Surrounding columns + the name descriptor for one row. The source
    /// pre-paints `left`; the engine paints `name`.
    fn row_cells(&self, idx: usize, ctx: &RenderCtx) -> RowCells;
    /// Extract key for the row, if it has one (a file path, a sheet name).
    /// `None` for rows that can't be extracted (tree directories).
    fn extract_target(&self, idx: usize) -> Option<ExtractTarget>;
    /// In-frame jump for the row: switch to the named sibling mode and seek
    /// it to `Position` (e.g. a symbol → its byte offset in the Hex view).
    /// `None` (default) means the row descends / extracts instead.
    fn jump_target(&self, _idx: usize) -> Option<(ModeId, Position)> {
        None
    }
    /// One `--list` line for the row, or `None` to omit it (tree dirs).
    fn flat_line(&self, idx: usize, theme: &PeekTheme) -> Option<String>;
    /// Status-segment label, e.g. "ZIP" / "directory".
    fn source_label(&self) -> &str;
}

/// Render bundle for a single row. `left` cells are pre-painted and
/// pre-padded by the source; `prefix` carries tree connectors (painted
/// muted by the engine); `name` is painted by the engine so it can overlay
/// search + selection state.
pub struct RowCells {
    pub prefix: String,
    pub left: Vec<String>,
    pub name: NameCell,
}

/// The one selectable column. `is_dir` picks the accent colour and the
/// trailing `/`; `text` is what the engine paints, overlays matches on, and
/// backgrounds when selected.
pub struct NameCell {
    pub text: String,
    pub is_dir: bool,
}

/// Navigation metadata the engine caches per row so the viewport can do
/// scroll / sticky / selection math without re-querying the source on every
/// tick. Built once from [`ListSource::parent`] / [`ListSource::selectable`].
pub(super) struct RowMetaCell {
    pub parent: Option<usize>,
    pub selectable: bool,
}

impl RowMeta for RowMetaCell {
    fn parent(&self) -> Option<usize> {
        self.parent
    }
    fn selectable(&self) -> bool {
        self.selectable
    }
}
