//! Filesystem directory viewer. One flat level — selecting a child
//! file descends into peek; selecting a child directory re-targets the
//! current frame so there's no stack of dirs to back out of.

pub mod compose;
pub mod extract;
pub mod info;
pub mod mode;
pub mod read;

pub use mode::DirectoryMode;
