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
//! ## Facade
//!
//! The historical in-crate paths are preserved so the moved type modules
//! keep compiling unchanged: `crate::theme` / `crate::input` /
//! `crate::viewer` / `crate::info` / `crate::output` / `crate::base64` /
//! `crate::xml` all resolve here, re-exported from `peek-foundation` (and
//! `peek-theme` / `peek-io` / `peek-detect`).

pub use peek_theme as theme;

// The reader/viewer foundation, re-exported under the historical paths the
// type modules use.
pub use peek_foundation::{base64, extract, info, output, viewer, xml};
// `impl_info_extras!` is `#[macro_export]`ed at the foundation crate root;
// re-export so `crate::impl_info_extras!` resolves in the type modules.
pub use peek_foundation::impl_info_extras;

/// Thin façade over `peek-io` + `peek-detect`, mirroring the foundation's
/// own `input` façade so `crate::input::*` resolves here too.
pub mod input {
    /// `crate::input::limits` — the memory-budget classes every size
    /// gate aliases.
    pub use peek_io::limits;
    pub use peek_io::{ByteSource, InputSource, LineSource};
    pub use peek_io::{source, stream};

    pub mod detect {
        pub use peek_detect::*;
    }

    pub use peek_detect::mime;

    pub mod compression {
        pub use peek_detect::resolve_transparent;
        pub use peek_io::compression::MAX_SPILL_BYTES;
    }
}

pub mod types;

#[cfg(test)]
mod derive_view_tests;
