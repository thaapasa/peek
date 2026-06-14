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

use crate::viewer::modes::{ExtractTarget, ModeId, Position, RenderCtx};
use crate::viewer::ui::HelpEntry;
use peek_theme::PeekTheme;

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
    /// Declared (uncompressed) size of the row's extractable payload, if
    /// known — used to gate the slow extract behind a confirmation prompt.
    /// `None` (default) when the source has no size or the row isn't
    /// extractable; the gate then lets the extract proceed unprompted.
    fn extract_size(&self, _idx: usize) -> Option<u64> {
        None
    }
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
    /// Which engine actions are live for this source — drives the help
    /// screen only ([`super::mode::ListingMode`]'s dispatch card stays
    /// static; an inert key is a harmless no-op). The default matches
    /// the tree shape (extractable rows under parent directories); flat
    /// and jump-select sources override.
    fn help(&self) -> ListingHelp {
        ListingHelp::default()
    }
    /// Preferred row to seed the selection on, or `None` to start on the
    /// first selectable row. Tree TOCs start on the first *file* so opening
    /// a container lands on a descendable entry even though directory rows
    /// are selectable too (for `Backspace` / future folding).
    fn initial_selection(&self) -> Option<usize> {
        None
    }
    /// How this source answers the parent-directory key. The default —
    /// `InListing` — moves the selection up a level within the current
    /// rows (the tree-TOC shape: the listing is fixed, there's no frame to
    /// navigate to). The on-disk directory browser overrides with
    /// `Descend("..")` so the session opens the real parent directory.
    fn parent_nav(&self) -> ListParentNav {
        ListParentNav::InListing
    }
}

/// How a [`ListSource`] responds to the parent-directory key.
pub enum ListParentNav {
    /// Move the selection up one level inside the current rows (tree TOC).
    InListing,
    /// Descend the session into this extract key — the on-disk `..` row.
    Descend(String),
}

/// Help-screen descriptor a [`ListSource`] declares so the listing
/// engine advertises only the actions that can visibly do something in
/// this view. Declared, not inferred from rows — a source states its
/// select semantics directly.
pub struct ListingHelp {
    /// Rows extract (`Action::Extract` reaches [`ListSource::extract_target`]).
    pub extract: bool,
    /// Rows have parents, so the sticky-breadcrumb toggle is visible.
    pub sticky: bool,
    /// Source-specific select semantic, e.g. the symbol list's
    /// `(Descend, "Jump to symbol in hex")`. `None` when selecting just
    /// descends/extracts (already covered by the global help entries).
    pub select: Option<HelpEntry>,
}

impl Default for ListingHelp {
    fn default() -> Self {
        Self {
            extract: true,
            sticky: true,
            select: None,
        }
    }
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
