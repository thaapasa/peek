//! peek input layer.
//!
//! The "stream, don't load" foundation: [`InputSource`] (File / Memory /
//! FileRange / TempFile) plus the byte/line streaming primitives layered
//! on top of it, and the bare single-stream decompression codecs. This
//! crate is dependency-free of the detection and reader/viewer layers —
//! everything above builds on it.

pub mod compression;
pub mod lines;
pub mod source;
pub mod stdin;
pub mod stream;

pub use lines::LineSource;
pub use source::{ByteSource, InputSource};
pub use stream::ByteStream;
