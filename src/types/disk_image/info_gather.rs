//! Disk-image info gathering. Each format reads only its metadata
//! region — ISO pulls the 16 KiB descriptor area at offset 32768; DMG
//! pulls the trailing 512-byte UDIF trailer. Multi-GB images stay
//! cheap because no payload bytes are touched.

use bytes::Bytes;

use super::{dmg_plist, dmg_trailer, iso_pvd, mbr, mish};
use crate::info::FileExtras;
use crate::input::InputSource;
use crate::input::detect::DiskImageFormat;
use crate::types::disk_image::info::{
    DiskImageInfo, DiskImageMeta, DmgMeta, DmgPartition, RawImageMeta,
};

/// Sectors of the descriptor area we pull on a single read for ISO.
/// Eight 2 KiB sectors covers PVD + supplementary descriptors + boot
/// record + terminator with comfortable headroom.
const ISO_DESCRIPTOR_READ_BYTES: usize = 8 * 2048;

pub fn gather_extras(source: &InputSource, fmt: DiskImageFormat) -> FileExtras {
    let format_name = fmt.label();
    match fmt {
        DiskImageFormat::Iso => gather_iso(source, format_name),
        DiskImageFormat::Dmg => gather_dmg(source, format_name),
        DiskImageFormat::Raw => gather_raw(source, format_name),
    }
}

fn gather_raw(source: &InputSource, format_name: &'static str) -> FileExtras {
    // Read just one boot sector (512 bytes) — that's enough for an
    // MBR partition table. Anything bigger would be needed only by
    // a GPT walker, which isn't implemented here.
    let mbr = source
        .open_byte_source()
        .ok()
        .and_then(|bs| bs.read_range(0, mbr::BOOT_SECTOR_BYTES).ok())
        .and_then(|buf| mbr::parse(&buf));
    FileExtras::DiskImage(DiskImageInfo {
        format_name,
        meta: Some(DiskImageMeta::Raw(RawImageMeta { mbr })),
        error: None,
    })
}

fn gather_iso(source: &InputSource, format_name: &'static str) -> FileExtras {
    match read_iso_descriptors(source) {
        Ok(buf) => match iso_pvd::parse(&buf) {
            Some(iso) => FileExtras::DiskImage(DiskImageInfo {
                format_name,
                meta: Some(DiskImageMeta::Iso(iso)),
                error: None,
            }),
            None => FileExtras::DiskImage(DiskImageInfo {
                format_name,
                meta: None,
                error: Some(
                    "not a valid ISO 9660 image (Primary Volume Descriptor missing)".into(),
                ),
            }),
        },
        Err(e) => FileExtras::DiskImage(DiskImageInfo {
            format_name,
            meta: None,
            error: Some(format!("{e:#}")),
        }),
    }
}

fn gather_dmg(source: &InputSource, format_name: &'static str) -> FileExtras {
    match read_dmg_trailer(source) {
        Ok(buf) => match dmg_trailer::parse(&buf) {
            Some(mut dmg) => {
                // Decode the partition map from the embedded plist. Best
                // effort: a parse failure leaves the trailer info intact.
                if dmg.plist_present {
                    dmg.partitions = read_dmg_partitions(source, &dmg).unwrap_or_default();
                }
                FileExtras::DiskImage(DiskImageInfo {
                    format_name,
                    meta: Some(DiskImageMeta::Dmg(dmg)),
                    error: None,
                })
            }
            None => FileExtras::DiskImage(DiskImageInfo {
                format_name,
                meta: None,
                error: Some("not a UDIF disk image (koly trailer signature missing)".into()),
            }),
        },
        Err(e) => FileExtras::DiskImage(DiskImageInfo {
            format_name,
            meta: None,
            error: Some(format!("{e:#}")),
        }),
    }
}

fn read_iso_descriptors(source: &InputSource) -> anyhow::Result<Bytes> {
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

fn read_dmg_trailer(source: &InputSource) -> anyhow::Result<Bytes> {
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

/// Upper bound on the embedded plist we'll read into memory. Real DMG
/// plists run a few KB to low single-digit MB; this only guards a corrupt
/// length field from driving a huge allocation.
const DMG_PLIST_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Read the embedded XML plist and decode its blkx tables into partition
/// rows. Touches only the plist region — no payload bytes.
fn read_dmg_partitions(source: &InputSource, dmg: &DmgMeta) -> anyhow::Result<Vec<DmgPartition>> {
    if dmg.plist_length == 0 || dmg.plist_length > DMG_PLIST_MAX_BYTES {
        anyhow::bail!("implausible plist length: {}", dmg.plist_length);
    }
    let bs = source.open_byte_source()?;
    let end = dmg
        .plist_offset
        .checked_add(dmg.plist_length)
        .ok_or_else(|| anyhow::anyhow!("plist range overflows u64"))?;
    if end > bs.len() {
        anyhow::bail!(
            "plist range {}..{} exceeds image size {}",
            dmg.plist_offset,
            end,
            bs.len()
        );
    }
    let raw = bs.read_range(dmg.plist_offset, dmg.plist_length as usize)?;
    let xml =
        std::str::from_utf8(&raw).map_err(|e| anyhow::anyhow!("plist is not valid UTF-8: {e}"))?;
    Ok(dmg_plist::extract_blkx(xml)
        .into_iter()
        .filter_map(build_partition)
        .collect())
}

/// Turn one blkx entry into a partition row, reading size + compression
/// from its mish block table. `None` when the entry's `Data` isn't a
/// valid block table.
fn build_partition(entry: dmg_plist::BlkxEntry) -> Option<DmgPartition> {
    let summary = mish::MishTable::parse(&entry.data)?.summary();
    Some(DmgPartition {
        fs_type: parse_fs_type(&entry.name),
        name: entry.name,
        size_bytes: summary.uncompressed_bytes,
        stored_bytes: summary.stored_bytes,
        compression: summary.methods,
        chunk_count: summary.chunk_count,
    })
}

/// Pull the Apple partition-type token out of a blkx name like
/// `"disk image (Apple_HFS : 4)"` → `"Apple_HFS"`. Returns `None` when
/// the name carries no parenthesised `type : index` form.
fn parse_fs_type(name: &str) -> Option<String> {
    let open = name.rfind('(')?;
    let inner = &name[open + 1..];
    let close = inner.find(')')?;
    let token = inner[..close].split(':').next()?.trim();
    (!token.is_empty()).then(|| token.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_fs_type;

    #[test]
    fn extracts_type_token_from_blkx_name() {
        assert_eq!(
            parse_fs_type("disk image (Apple_HFS : 4)").as_deref(),
            Some("Apple_HFS")
        );
        assert_eq!(
            parse_fs_type("Protective Master Boot Record (MBR : 0)").as_deref(),
            Some("MBR")
        );
        // Multi-word token before the colon is kept intact.
        assert_eq!(
            parse_fs_type("GPT Header (Primary GPT Header : 1)").as_deref(),
            Some("Primary GPT Header")
        );
    }

    #[test]
    fn none_without_parenthesised_type() {
        assert_eq!(parse_fs_type("just a name"), None);
        assert_eq!(parse_fs_type("trailing ("), None);
        assert_eq!(parse_fs_type("( : 3)"), None);
    }
}
