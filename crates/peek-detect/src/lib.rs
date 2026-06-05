//! peek file-type detection.
//!
//! Owns [`FileType`] and every per-type format enum, plus the
//! magic-byte / extension / content-sniff classification that maps an
//! [`InputSource`](peek_io::InputSource) to a [`Detected`]. Built on
//! `peek-io` only — it never depends on the reader / viewer layer, so the
//! whole detection surface can be reviewed and fuzzed in isolation.
//!
//! The per-type modules under [`types`] hold the format enums and pure
//! sniff helpers; the `peek` binary's `types/` readers import those
//! format enums back through this crate.

pub mod detect;
pub mod mime;
mod transparent;
pub mod types;

pub use detect::*;
pub use transparent::resolve_transparent;
