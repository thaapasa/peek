//! peek per-file-type readers.
//!
//! One module per file type under [`types`], each owning its reader,
//! info-gathering, and view-mode construction. The format enums and pure
//! sniff helpers live in `peek-detect` (re-exported at each module root);
//! this crate is the parsing + presentation layer built on the
//! [`peek-foundation`](peek_foundation) toolkit.
//!
//! Cargo bars this crate from naming the binary's session layer (the
//! `compose` / `gather` dispatch hubs and the interactive event loop), so
//! a parser bug here can never reach process / terminal control. The
//! binary wires the per-type `compose` / `extract` / `gather_extras`
//! functions into its dispatch hubs.
//!
//! The leaf crates are named directly — `peek_io`, `peek_detect`,
//! `peek_theme`. The reader/viewer toolkit this crate is built on is
//! re-exported from `peek-foundation` under the in-crate paths
//! `crate::{base64, extract, info, output, viewer, xml}`.

// The reader/viewer foundation, re-exported under the in-crate paths the
// type modules use.
// `impl_info_extras!` is `#[macro_export]`ed at the foundation crate root;
// re-export so `crate::impl_info_extras!` resolves in the type modules.
pub use peek_foundation::impl_info_extras;
// Same for `info_section!`, which generates the per-type `render_section`
// / `json_section` pair that `impl_info_extras!`'s three-arg form wires.
pub use peek_foundation::info_section;
pub use peek_foundation::{base64, extract, info, output, viewer, xml};

pub mod types;

#[cfg(test)]
mod derive_view_tests;
