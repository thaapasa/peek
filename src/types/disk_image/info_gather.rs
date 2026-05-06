//! Disk-image info gathering. Reads only the volume descriptor area
//! (16 KiB starting at byte 32768) via `ByteSource::read_range`, so
//! multi-GB images are cheap to introspect.

use super::iso_pvd;
use crate::info::FileExtras;
use crate::input::InputSource;
use crate::input::detect::DiskImageFormat;

/// Sectors of the descriptor area we pull on a single read. Eight 2 KiB
/// sectors covers PVD + supplementary descriptors + boot record +
/// terminator with comfortable headroom.
const DESCRIPTOR_READ_BYTES: usize = 8 * 2048;

pub fn gather_extras(source: &InputSource, fmt: DiskImageFormat) -> FileExtras {
    let format_name = fmt.label();
    match fmt {
        DiskImageFormat::Iso => match read_descriptor_area(source) {
            Ok(buf) => match iso_pvd::parse(&buf) {
                Some(iso) => FileExtras::DiskImage {
                    format_name,
                    iso: Some(iso),
                    error: None,
                },
                None => FileExtras::DiskImage {
                    format_name,
                    iso: None,
                    error: Some(
                        "not a valid ISO 9660 image (Primary Volume Descriptor missing)".into(),
                    ),
                },
            },
            Err(e) => FileExtras::DiskImage {
                format_name,
                iso: None,
                error: Some(format!("{e:#}")),
            },
        },
    }
}

fn read_descriptor_area(source: &InputSource) -> anyhow::Result<Vec<u8>> {
    let bs = source.open_byte_source()?;
    if bs.len() <= iso_pvd::PVD_OFFSET {
        anyhow::bail!(
            "image is too small to contain a Primary Volume Descriptor ({} bytes < {})",
            bs.len(),
            iso_pvd::PVD_OFFSET + 2048
        );
    }
    let buf = bs.read_range(iso_pvd::PVD_OFFSET, DESCRIPTOR_READ_BYTES)?;
    if buf.len() < 2048 {
        anyhow::bail!(
            "descriptor area read returned {} bytes (< one sector)",
            buf.len()
        );
    }
    Ok(buf)
}
