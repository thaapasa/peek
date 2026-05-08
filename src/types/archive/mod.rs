//! Archive table-of-contents support.
//!
//! Listing-only — no payload extraction. Each backend reads only enough
//! of the archive structure (zip central directory, tar header chain,
//! 7z header) to enumerate entries with size, mtime, and unix mode.
//! Backends produce flat path-keyed [`crate::types::listing::FlatEntry`]
//! lists; the shared [`crate::types::listing`] module turns them into a
//! tree and renders the interactive TOC view.

mod backends;
pub mod extract;
pub mod info;
pub mod reader;
