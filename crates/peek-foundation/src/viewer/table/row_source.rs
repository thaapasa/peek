//! Backing-data abstraction for [`RowsTableMode`].
//!
//! [`RowsTableMode`] is the lazy / streaming variant of the table view
//! (sticky header + horizontal pan + cell-scoped search). It does not
//! own its data — instead it talks to a [`RowSource`] that lets it pull
//! more rows on demand and read out individual cells.
//!
//! Two concrete sources live on top of this trait today:
//!
//! * `CsvData` — streams records out of a
//!   `csv::Reader`, holding a seed of the first records plus a sliding
//!   window over wherever the user scrolled past it; memory stays flat
//!   regardless of file size.
//! * `SqliteRowSet` (added by the SQLite viewer) — windowed read out of
//!   a `rusqlite` connection: `ensure_row` slides a buffer covering the
//!   current viewport, total row count is known up front.
//!
//! Cells are `Option<String>` so NULL stays distinguishable from the
//! empty string (SQL semantics). CSV always produces `Some(_)` cells;
//! the cost is one extra `Option` per cell, which we accept for the
//! cross-source uniformity.
//!
//! Errors during row pulls are reported as malformed rows (CSV parse
//! errors today; future sources may produce them too) rather than
//! bubbled — the table view should keep rendering the rest of the data.
//! [`RowSource::row_is_malformed`] picks them out; the cell payload is
//! ignored for malformed rows.
//!
//! [`RowsTableMode`]: super::rows_mode::RowsTableMode

use anyhow::Result;

/// Lazy, index-addressable row stream.
///
/// Implementations cache rows as the caller asks for them, but both
/// concrete sources keep memory bounded: `CsvData` holds a fixed seed
/// plus a sliding window, `SqliteRowSet` a single window over a known
/// total. The mode treats every successful [`Self::row`] lookup as
/// ground truth and only ever calls [`Self::ensure_row`] beforehand for
/// rows it is about to render or scan.
pub trait RowSource {
    /// Make `idx` accessible if possible. Returns the current
    /// upper bound (`row(i)` will return `None` for any `i >=
    /// returned value`). For append-only sources this grows
    /// monotonically; for windowed sources it is the total row
    /// count.
    fn ensure_row(&mut self, idx: usize) -> Result<usize>;

    /// Drive the source to its end so [`Self::total`] becomes definite.
    /// May be a no-op when total is already known (windowed sources).
    fn ensure_all(&mut self) -> Result<()>;

    /// Cells of row `idx`. `None` when the row hasn't been materialised
    /// (either out of range or, for windowed sources, outside the
    /// current window — the caller is expected to call [`Self::ensure_row`]
    /// first when it needs the row).
    fn row(&self, idx: usize) -> Option<&[Option<String>]>;

    /// Whether row `idx` should be rendered as a `<error>` placeholder
    /// (CSV reader rejected it, future sources may flag others). Default
    /// is "no" — most sources only emit clean rows.
    fn row_is_malformed(&self, _idx: usize) -> bool {
        false
    }

    /// Number of rows currently accessible without further calls to
    /// [`Self::ensure_row`]. For windowed sources this is the total
    /// row count.
    fn loaded(&self) -> usize;

    /// Total row count if known, otherwise `None`. CSV reports `None`
    /// until the reader hits EOF; SQLite reports the cached `COUNT(*)`
    /// up front.
    fn total(&self) -> Option<usize>;

    /// Number of columns. Fixed for the lifetime of the source.
    fn column_count(&self) -> usize;

    /// Count of malformed rows seen so far. Surfaced in the status bar
    /// when non-zero. Default `0` for sources that can't produce them.
    fn malformed_count(&self) -> usize {
        0
    }
}
