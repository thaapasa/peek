//! Format-specific TOC decoders. Each backend reads only the structural
//! metadata (central directory / header chain / file index) needed to
//! enumerate `ArchiveEntry`s — no payload extraction.

pub(super) mod sevenz;
pub(super) mod tar;
pub(super) mod zip;
