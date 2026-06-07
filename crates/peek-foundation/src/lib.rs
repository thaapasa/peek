//! peek reader/viewer foundation.
//!
//! The shared toolkit every file-type module builds on: the [`viewer`]
//! mode framework (the `Mode` trait, `RenderCtx`, the shared view modes,
//! the terminal UI primitives, the image-render vocabulary), the [`info`]
//! base (the `InfoExtras` trait + `FileInfo` + the section renderer), the
//! [`output`] pipe writer, and the shared [`base64`] / [`xml`] helpers.
//!
//! It sits above `peek-theme` / `peek-io` / `peek-detect` and below
//! `peek-types`. Cargo bars it from naming the binary's session layer
//! (the `compose` / `gather` dispatch hubs and the interactive event
//! loop), so a parser bug in `peek-types` can never reach process /
//! terminal control through this crate.
//!
//! ## Facade
//!
//! The historical in-crate paths are preserved so the moved modules — and
//! `peek-types`, which depends on this crate — keep compiling unchanged:
//!
//! - `crate::theme::*` → the `peek-theme` crate (aliased here).
//! - `crate::input::*` → the `peek-io` + `peek-detect` re-export façade,
//!   mirroring the binary's own `src/input` façade.

// So the `#[derive(InfoView)]` macro's fully-qualified `::peek_foundation::…`
// paths resolve when the derive is used inside this crate too (it generates
// the same absolute paths regardless of call site).
extern crate self as peek_foundation;

pub use peek_theme as theme;

/// Thin façade over `peek-io` + `peek-detect`, re-exporting them under the
/// historical `crate::input::*` paths the reader/viewer layer uses. Mirrors
/// the binary's own `src/input` façade (minus the CLI-level stdin dispatch,
/// which needs `Args` and stays in the bin).
pub mod input {
    pub use peek_io::{ByteSource, InputSource, LineSource};
    pub use peek_io::{source, stream};

    /// `crate::input::detect::*` — the detection surface.
    pub mod detect {
        pub use peek_detect::*;
    }

    /// `crate::input::mime` — MIME classification helpers.
    pub use peek_detect::mime;

    /// `crate::input::compression` — transparent decompress-then-redetect.
    pub mod compression {
        pub use peek_detect::resolve_transparent;
    }
}

pub mod base64;
pub mod extract;
pub mod info;
pub mod output;
pub mod viewer;
pub mod xml;
