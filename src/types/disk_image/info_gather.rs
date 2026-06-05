//! Disk-image info gathering. Each format reads only its metadata
//! region — ISO pulls the 16 KiB descriptor area at offset 32768; DMG
//! pulls the trailing 512-byte UDIF trailer. Multi-GB images stay
//! cheap because no payload bytes are touched.

use bytes::Bytes;

use super::{dmg_plist, dmg_trailer, iso_pvd, mbr, mish};
use crate::info::Extras;
use crate::input::InputSource;
use crate::input::detect::DiskImageFormat;
use crate::types::disk_image::info::{
    DiskImageInfo, DiskImageMeta, DmgMeta, DmgPartition, RawImageMeta,
};

/// Sectors of the descriptor area we pull on a single read for ISO.
/// Eight 2 KiB sectors covers PVD + supplementary descriptors + boot
/// record + terminator with comfortable headroom.
const ISO_DESCRIPTOR_READ_BYTES: usize = 8 * 2048;

pub fn gather_extras(source: &InputSource, fmt: DiskImageFormat) -> Extras {
    let format_name = fmt.label();
    match fmt {
        DiskImageFormat::Iso => gather_iso(source, format_name),
        DiskImageFormat::Dmg => gather_dmg(source, format_name),
        DiskImageFormat::Raw => gather_raw(source, format_name),
    }
}

fn gather_raw(source: &InputSource, format_name: &'static str) -> Extras {
    // Read just one boot sector (512 bytes) — that's enough for an
    // MBR partition table. Anything bigger would be needed only by
    // a GPT walker, which isn't implemented here.
    let mbr = source
        .open_byte_source()
        .ok()
        .and_then(|bs| bs.read_range(0, mbr::BOOT_SECTOR_BYTES).ok())
        .and_then(|buf| mbr::parse(&buf));
    Box::new(DiskImageInfo {
        format_name,
        meta: Some(DiskImageMeta::Raw(RawImageMeta { mbr })),
        error: None,
    })
}

fn gather_iso(source: &InputSource, format_name: &'static str) -> Extras {
    match read_iso_descriptors(source) {
        Ok(buf) => match iso_pvd::parse(&buf) {
            Some(iso) => Box::new(DiskImageInfo {
                format_name,
                meta: Some(DiskImageMeta::Iso(iso)),
                error: None,
            }),
            None => Box::new(DiskImageInfo {
                format_name,
                meta: None,
                error: Some(
                    "not a valid ISO 9660 image (Primary Volume Descriptor missing)".into(),
                ),
            }),
        },
        Err(e) => Box::new(DiskImageInfo {
            format_name,
            meta: None,
            error: Some(format!("{e:#}")),
        }),
    }
}

fn gather_dmg(source: &InputSource, format_name: &'static str) -> Extras {
    match read_dmg_trailer(source) {
        Ok(buf) => match dmg_trailer::parse(&buf) {
            Some(mut dmg) => {
                // Decode the partition map from the embedded plist. Best
                // effort: a parse failure leaves the trailer info intact.
                if dmg.plist_present {
                    dmg.partitions = read_dmg_partitions(source, &dmg).unwrap_or_default();
                }
                Box::new(DiskImageInfo {
                    format_name,
                    meta: Some(DiskImageMeta::Dmg(dmg)),
                    error: None,
                })
            }
            None => Box::new(DiskImageInfo {
                format_name,
                meta: None,
                error: Some("not a UDIF disk image (koly trailer signature missing)".into()),
            }),
        },
        Err(e) => Box::new(DiskImageInfo {
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
        start_sector: summary.first_sector,
        size_bytes: summary.uncompressed_bytes,
        stored_bytes: summary.stored_bytes,
        compression: summary.codecs(),
        chunk_count: summary.chunk_count,
        run_histogram: summary.run_counts,
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
    use super::*;

    // --- synthetic-DMG builders ------------------------------------------
    // Mirror the on-disk shapes just enough to drive the real gather path:
    // a "mish" block table, a base64 encoder (crate::base64 only decodes),
    // an embedding plist, and a koly trailer pointing at it.

    /// Build a mish block: `(tag, stored_length)` chunks plus the fixed
    /// header carrying start sector + sector span.
    fn mish(first_sector: u64, sector_count: u64, chunks: &[(u32, u64)]) -> Vec<u8> {
        const HEADER: usize = 204;
        const CHUNK: usize = 40;
        let mut b = vec![0u8; HEADER + chunks.len() * CHUNK];
        b[0..4].copy_from_slice(b"mish");
        b[8..16].copy_from_slice(&first_sector.to_be_bytes());
        b[16..24].copy_from_slice(&sector_count.to_be_bytes());
        b[200..204].copy_from_slice(&(chunks.len() as u32).to_be_bytes());
        for (i, (tag, stored)) in chunks.iter().enumerate() {
            let off = HEADER + i * CHUNK;
            b[off..off + 4].copy_from_slice(&tag.to_be_bytes());
            b[off + 32..off + 40].copy_from_slice(&stored.to_be_bytes());
        }
        b
    }

    /// Standard-alphabet base64 encode (round-trips with crate::base64).
    fn b64(data: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);
            out.push(A[(b0 >> 2) as usize] as char);
            out.push(A[(((b0 & 0x3) << 4) | (b1 >> 4)) as usize] as char);
            out.push(if chunk.len() > 1 {
                A[(((b1 & 0xf) << 2) | (b2 >> 6)) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                A[(b2 & 0x3f) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    /// 512-byte koly trailer pointing at the embedded plist.
    fn koly(plist_offset: u64, plist_length: u64, sector_count: u64) -> Vec<u8> {
        let mut b = vec![0u8; 512];
        b[0..4].copy_from_slice(b"koly");
        b[4..8].copy_from_slice(&4u32.to_be_bytes()); // version
        b[216..224].copy_from_slice(&plist_offset.to_be_bytes());
        b[224..232].copy_from_slice(&plist_length.to_be_bytes());
        b[488..492].copy_from_slice(&1u32.to_be_bytes()); // device variant
        b[492..500].copy_from_slice(&sector_count.to_be_bytes());
        b
    }

    fn blkx_entry(name: &str, data: &[u8]) -> String {
        format!(
            "<dict><key>Data</key><data>{}</data>\
             <key>Name</key><string>{name}</string></dict>",
            b64(data)
        )
    }

    #[test]
    fn build_partition_reads_mish_into_row() {
        // 2048 sectors logical, one zlib chunk storing 4 KiB + terminator.
        let data = mish(40, 2048, &[(0x8000_0005, 4096), (0xffff_ffff, 0)]);
        let entry = dmg_plist::BlkxEntry {
            name: "disk image (Apple_HFS : 4)".to_string(),
            data,
        };
        let p = build_partition(entry).expect("valid mish → partition");
        assert_eq!(p.fs_type.as_deref(), Some("Apple_HFS"));
        assert_eq!(p.start_sector, 40);
        assert_eq!(p.size_bytes, 2048 * 512);
        assert_eq!(p.stored_bytes, 4096);
        assert_eq!(p.compression, vec!["zlib"]);
        assert_eq!(p.chunk_count, 1);
        assert_eq!(p.run_histogram, vec![("zlib", 1)]);
    }

    #[test]
    fn build_partition_rejects_non_mish_data() {
        let entry = dmg_plist::BlkxEntry {
            name: "x".to_string(),
            data: b"not a mish block".to_vec(),
        };
        assert!(build_partition(entry).is_none());
    }

    #[test]
    fn gather_dmg_decodes_partition_map_end_to_end() {
        let mbr = mish(0, 1, &[(0x8000_0005, 30), (0xffff_ffff, 0)]);
        let hfs = mish(40, 2048, &[(0x8000_0005, 4096), (0xffff_ffff, 0)]);
        let plist = format!(
            "<?xml version=\"1.0\"?><plist><dict>\
             <key>resource-fork</key><dict>\
             <key>blkx</key><array>{}{}</array>\
             </dict></dict></plist>",
            blkx_entry("Protective Master Boot Record (MBR : 0)", &mbr),
            blkx_entry("disk image (Apple_HFS : 4)", &hfs),
        );

        // Layout: [data-fork stand-in][plist][koly]. The plist offset must
        // be non-zero (the trailer's plist_present check rejects offset 0).
        let mut file = vec![0u8; 256];
        let plist_offset = file.len() as u64;
        file.extend_from_slice(plist.as_bytes());
        file.extend_from_slice(&koly(plist_offset, plist.len() as u64, 2089));

        let source = InputSource::memory(file, "synthetic.dmg");
        let extras = gather_extras(&source, DiskImageFormat::Dmg);
        let info = crate::info::downcast_extras::<DiskImageInfo>(&extras);
        let Some(DiskImageMeta::Dmg(dmg)) = &info.meta else {
            panic!("expected Dmg meta, error = {:?}", info.error);
        };

        assert_eq!(dmg.partitions.len(), 2, "both blkx entries decoded");
        // Order preserved from the plist.
        assert_eq!(dmg.partitions[0].fs_type.as_deref(), Some("MBR"));
        let hfs = &dmg.partitions[1];
        assert_eq!(hfs.fs_type.as_deref(), Some("Apple_HFS"));
        assert_eq!(hfs.start_sector, 40);
        assert_eq!(hfs.size_bytes, 2048 * 512);
        assert_eq!(hfs.stored_bytes, 4096);
        assert_eq!(hfs.compression, vec!["zlib"]);
    }

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
