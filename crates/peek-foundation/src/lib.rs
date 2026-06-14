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
//! Code names the lower crates directly — `peek_io`, `peek_detect`,
//! `peek_theme` — rather than through an in-crate re-export façade.

// So the `#[derive(InfoView)]` macro's fully-qualified `::peek_foundation::…`
// paths resolve when the derive is used inside this crate too (it generates
// the same absolute paths regardless of call site).
extern crate self as peek_foundation;

/// Macro-support alias only: the `#[derive(InfoView)]` expansion emits
/// `::peek_foundation::theme::PeekTheme`, routed through this crate because
/// `peek-foundation` is the one dependency every deriving crate is
/// guaranteed to have. Hand-written code names `peek_theme` directly — do
/// not reach for `crate::theme`.
pub use peek_theme as theme;

pub mod base64;
pub mod extract;
pub mod info;
pub mod output;
pub mod viewer;
pub mod xml;
