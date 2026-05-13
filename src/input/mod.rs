pub mod compression;
pub mod detect;
pub mod lines;
pub mod mime;
pub mod source;
pub mod stdin;
pub mod stream;

pub use lines::LineSource;
pub use source::{ByteSource, InputSource};
pub use stream::ByteStream;
