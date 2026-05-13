//! Archive table-of-contents support.
//!
//! Listing-only — no payload extraction. Each backend reads only enough
//! of the archive structure (zip central directory, tar header chain,
//! 7z header) to enumerate entries with size, mtime, and unix mode.
//! Backends produce flat path-keyed [`crate::viewer::listing::FlatEntry`]
//! lists; the shared [`crate::viewer::listing`] module turns them into a
//! tree and renders the interactive TOC view.

mod backends;
pub mod extract;
pub mod info;
pub mod reader;
