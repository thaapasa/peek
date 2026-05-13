//! Disk-image detection: extension classifier plus the
//! `Raw → Iso` upgrade probe (cheap 6-byte read at the PVD offset
//! 32768 to look for the `\x01CD001` signature).

use std::fs;
use std::path::Path;

use super::format::DiskImageFormat;

/// Map a single file extension to a disk-image format. `.img` /
/// `.bin` / `.dd` map to `Raw` provisionally; the orchestrator runs
/// [`upgrade_raw_to_iso_path`] / [`upgrade_raw_to_iso_bytes`] to
/// upgrade to `Iso` when the body carries the ISO 9660 PVD.
pub fn format_from_ext(ext: &str) -> Option<DiskImageFormat> {
    match ext {
        "iso" => Some(DiskImageFormat::Iso),
        "dmg" => Some(DiskImageFormat::Dmg),
        "img" | "bin" | "dd" => Some(DiskImageFormat::Raw),
        _ => None,
    }
}

/// Upgrade a `Raw` classification to `Iso` when an in-memory byte
/// buffer carries an ISO 9660 PVD at offset 32768. No-op for any
/// non-`Raw` input.
pub fn upgrade_raw_to_iso_bytes(format: DiskImageFormat, data: &[u8]) -> DiskImageFormat {
    if matches!(format, DiskImageFormat::Raw)
        && data.len() >= 32774
        && data[32768] == 1
        && &data[32769..32774] == b"CD001"
    {
        return DiskImageFormat::Iso;
    }
    format
}

/// Path form of [`upgrade_raw_to_iso_bytes`] — opens the file and
/// reads the 6-byte PVD signature without slurping the whole image.
pub fn upgrade_raw_to_iso_path(format: DiskImageFormat, path: &Path) -> DiskImageFormat {
    if matches!(format, DiskImageFormat::Raw)
        && matches!(probe_iso(path), Some(DiskImageFormat::Iso))
    {
        return DiskImageFormat::Iso;
    }
    format
}

/// Read 6 bytes at the ISO 9660 PVD location (offset 32768 + 0..=5)
/// and check for the `\x01CD001` signature. Returns
/// `Some(DiskImageFormat::Iso)` on match, `None` otherwise (caller
/// keeps the `Raw` classification).
fn probe_iso(path: &Path) -> Option<DiskImageFormat> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(32768)).ok()?;
    let mut buf = [0u8; 6];
    file.read_exact(&mut buf).ok()?;
    if buf[0] == 1 && &buf[1..6] == b"CD001" {
        Some(DiskImageFormat::Iso)
    } else {
        None
    }
}
