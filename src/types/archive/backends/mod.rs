//! Format-specific TOC decoders. Each backend reads only the structural
//! metadata (central directory / header chain / file index) needed to
//! enumerate `ArchiveEntry`s — no payload extraction.

pub(super) mod ar;
pub(super) mod cpio;
pub(super) mod sevenz;
pub(super) mod single_stream;
pub(super) mod tar;
pub(super) mod zip;
