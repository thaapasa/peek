//! Disk-image metadata support (ISO 9660 + DMG).
//!
//! Volume-descriptor / trailer parsing plus the DMG partition map — no
//! inner-filesystem walk, no payload decompression.
//! `info_gather::gather_extras` reads just the descriptor area / trailer
//! (and, for DMG, the embedded plist) via `ByteSource::read_range`, so
//! multi-GB images are cheap to introspect.

pub mod compose;
pub mod detect;
pub mod dmg_plist;
pub mod dmg_trailer;
pub mod extract;
pub mod format;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod iso_listing;
pub mod iso_pvd;
pub mod mbr;
pub mod mish;
