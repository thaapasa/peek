//! Disk-image metadata support (ISO 9660 today; DMG planned).
//!
//! Volume-descriptor / trailer parsing only — no filesystem walk, no
//! payload extraction. `info_gather::gather_extras` reads just the
//! volume descriptor area via `ByteSource::read_range` so multi-GB
//! images are cheap to introspect.

pub mod dmg_trailer;
pub mod info_gather;
pub mod info_render;
pub mod iso_pvd;
