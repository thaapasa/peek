//! Disk-image info gathering. Each format reads only its metadata
//! region — ISO pulls the 16 KiB descriptor area at offset 32768; DMG
//! pulls the trailing 512-byte UDIF trailer. Multi-GB images stay
//! cheap because no payload bytes are touched.

use super::{dmg_trailer, iso_pvd};
use crate::info::{DiskImageMeta, FileExtras};
use crate::input::InputSource;
use crate::input::detect::DiskImageFormat;

/// Sectors of the descriptor area we pull on a single read for ISO.
/// Eight 2 KiB sectors covers PVD + supplementary descriptors + boot
/// record + terminator with comfortable headroom.
const ISO_DESCRIPTOR_READ_BYTES: usize = 8 * 2048;

pub fn gather_extras(source: &InputSource, fmt: DiskImageFormat) -> FileExtras {
    let format_name = fmt.label();
    match fmt {
        DiskImageFormat::Iso => gather_iso(source, format_name),
        DiskImageFormat::Dmg => gather_dmg(source, format_name),
    }
}

fn gather_iso(source: &InputSource, format_name: &'static str) -> FileExtras {
    match read_iso_descriptors(source) {
        Ok(buf) => match iso_pvd::parse(&buf) {
            Some(iso) => FileExtras::DiskImage {
                format_name,
                meta: Some(DiskImageMeta::Iso(iso)),
                error: None,
            },
            None => FileExtras::DiskImage {
                format_name,
                meta: None,
                error: Some(
                    "not a valid ISO 9660 image (Primary Volume Descriptor missing)".into(),
                ),
            },
        },
        Err(e) => FileExtras::DiskImage {
            format_name,
            meta: None,
            error: Some(format!("{e:#}")),
        },
    }
}

fn gather_dmg(source: &InputSource, format_name: &'static str) -> FileExtras {
    match read_dmg_trailer(source) {
        Ok(buf) => match dmg_trailer::parse(&buf) {
            Some(dmg) => FileExtras::DiskImage {
                format_name,
                meta: Some(DiskImageMeta::Dmg(dmg)),
                error: None,
            },
            None => FileExtras::DiskImage {
                format_name,
                meta: None,
                error: Some("not a UDIF disk image (koly trailer signature missing)".into()),
            },
        },
        Err(e) => FileExtras::DiskImage {
            format_name,
            meta: None,
            error: Some(format!("{e:#}")),
        },
    }
}

fn read_iso_descriptors(source: &InputSource) -> anyhow::Result<Vec<u8>> {
    let bs = source.open_byte_source()?;
    if bs.len() <= iso_pvd::PVD_OFFSET {
        anyhow::bail!(
            "image is too small to contain a Primary Volume Descriptor ({} bytes < {})",
            bs.len(),
            iso_pvd::PVD_OFFSET + 2048
        );
    }
    let buf = bs.read_range(iso_pvd::PVD_OFFSET, ISO_DESCRIPTOR_READ_BYTES)?;
    if buf.len() < 2048 {
        anyhow::bail!(
            "descriptor area read returned {} bytes (< one sector)",
            buf.len()
        );
    }
    Ok(buf)
}

fn read_dmg_trailer(source: &InputSource) -> anyhow::Result<Vec<u8>> {
    let bs = source.open_byte_source()?;
    let len = bs.len();
    if len < dmg_trailer::TRAILER_SIZE as u64 {
        anyhow::bail!(
            "image is too small to hold a UDIF trailer ({} bytes < {})",
            len,
            dmg_trailer::TRAILER_SIZE
        );
    }
    let offset = len - dmg_trailer::TRAILER_SIZE as u64;
    let buf = bs.read_range(offset, dmg_trailer::TRAILER_SIZE)?;
    if buf.len() != dmg_trailer::TRAILER_SIZE {
        anyhow::bail!(
            "tail read returned {} bytes (expected {})",
            buf.len(),
            dmg_trailer::TRAILER_SIZE
        );
    }
    Ok(buf)
}
