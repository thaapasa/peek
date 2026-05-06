//! Archive table-of-contents support.
//!
//! Listing-only — no payload extraction. Each backend reads only enough
//! of the archive structure (zip central directory, tar header chain,
//! 7z header) to enumerate entries with size, mtime, and unix mode.
//! `info::gather_extras` produces the info-view extras, `info::render_section`
//! draws them, and `mode::ArchiveMode` is the interactive TOC view.

mod backends;
pub mod info;
pub mod mode;
pub mod reader;

pub(crate) use mode::ArchiveMode;
