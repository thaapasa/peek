//! Generic table-of-contents abstraction for hierarchical container
//! types (archives, ISO 9660 disk images, future epub/msi/etc).
//!
//! Owns the data shapes (`Entry`, `EntryKind`, `EntryMtime`), the
//! aggregate `Stats`, and the interactive `ListingMode` that renders
//! a tree-style TOC with permissions, size, mtime, and path columns.
//! Callers either build an `Entry` tree natively (ISO walks
//! recursively) or via `from_flat_paths` for sources whose iterators
//! yield slash-delimited paths (zip, tar, 7z).

pub mod build;
pub mod entry;
pub mod mode;
pub mod stats;

pub use build::{FlatEntry, from_flat_paths};
pub use entry::{Entry, EntryKind, EntryMtime, time_from_epoch_secs};
pub use mode::ListingMode;
pub use stats::Stats;
