//! Thin façade over the extracted `peek-io` and `peek-detect` crates,
//! re-exporting them under the historical `crate::input::*` paths so the
//! reader / viewer layer keeps compiling unchanged. The only logic that
//! still lives here is the CLI-level source dispatch in [`stdin`] (which
//! depends on the binary's `Args`).
//!
//! - [`peek-io`](peek_io): `InputSource` + streaming byte/line sources +
//!   single-stream decompression codecs.
//! - [`peek-detect`](peek_detect): `FileType` + format enums + magic /
//!   extension / content classification + transparent decompression.

pub use peek_io::{ByteSource, InputSource};
pub use peek_io::{source, stream};

pub mod stdin;

/// `crate::input::detect::*` — the detection surface (`FileType`,
/// `Detected`, every `*Format` enum, `detect`, `detect_ignore_name`, …).
pub mod detect {
    pub use peek_detect::*;
}

/// `crate::input::mime` — MIME classification helpers.
pub use peek_detect::mime;

/// `crate::input::compression` — the transparent decompress-then-redetect
/// entry point. The bare codec primitives live in [`peek_io::compression`].
pub mod compression {
    pub use peek_detect::resolve_transparent;
}
