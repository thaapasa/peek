//! UDIF (Apple Disk Image) trailer parser.
//!
//! Every flat DMG ends in a 512-byte "koly" trailer with all the
//! structural metadata: format version, total sector count, data-fork
//! span, embedded plist offset, segment info, and checksum type tags.
//! All multi-byte fields are big-endian by Apple's spec.
//!
//! Header-only — the embedded XML plist (partition map / blkx tables)
//! and any per-block decoding live elsewhere. Reading this trailer is
//! one 512-byte tail read regardless of image size.

use crate::info::{DmgChecksumKind, DmgMeta, DmgVariant};

/// Trailer length, fixed by Apple's spec.
pub const TRAILER_SIZE: usize = 512;

const SIGNATURE: &[u8; 4] = b"koly";

/// Parse the trailing 512 bytes of a DMG. Returns `None` if the magic
/// signature is missing — caller surfaces this as "not a UDIF image".
pub fn parse(buf: &[u8]) -> Option<DmgMeta> {
    if buf.len() != TRAILER_SIZE || &buf[0..4] != SIGNATURE {
        return None;
    }
    let udif_version = read_u32(&buf[4..8]);
    let flags = read_u32(&buf[12..16]);
    let data_fork_length = read_u64(&buf[32..40]);
    let segment_number = read_u32(&buf[56..60]);
    let segment_count = read_u32(&buf[60..64]);
    let data_checksum_type = checksum_kind(read_u32(&buf[80..84]));
    let plist_offset = read_u64(&buf[216..224]);
    let plist_length = read_u64(&buf[224..232]);
    let master_checksum_type = checksum_kind(read_u32(&buf[352..356]));
    let variant = variant_from(read_u32(&buf[488..492]));
    let sector_count = read_u64(&buf[492..500]);

    Some(DmgMeta {
        udif_version,
        flags,
        variant,
        total_size_bytes: sector_count.saturating_mul(512),
        data_fork_length,
        plist_present: plist_offset != 0 && plist_length != 0,
        plist_length,
        segment_number,
        segment_count,
        data_checksum_type,
        master_checksum_type,
    })
}

fn read_u32(buf: &[u8]) -> u32 {
    u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
}

fn read_u64(buf: &[u8]) -> u64 {
    u64::from_be_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ])
}

fn variant_from(raw: u32) -> DmgVariant {
    match raw {
        1 => DmgVariant::Device,
        2 => DmgVariant::Partition,
        3 => DmgVariant::MountedSystem,
        other => DmgVariant::Other(other),
    }
}

/// Apple's documented checksum-type tags. Values 0 / 2 / 17 / 19 / 22
/// are the ones in active use; older tools occasionally emit the older
/// MD5 tag (1) as well, hence the explicit alias to `Md5`.
fn checksum_kind(raw: u32) -> DmgChecksumKind {
    match raw {
        0 => DmgChecksumKind::None,
        1 | 2 => DmgChecksumKind::Md5,
        17 => DmgChecksumKind::Crc32,
        19 => DmgChecksumKind::Sha1,
        22 => DmgChecksumKind::Sha256,
        24 => DmgChecksumKind::Sha512,
        other => DmgChecksumKind::Other(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_trailer(variant: u32, sectors: u64, plist_off: u64, plist_len: u64) -> Vec<u8> {
        let mut buf = vec![0u8; TRAILER_SIZE];
        buf[0..4].copy_from_slice(SIGNATURE);
        buf[4..8].copy_from_slice(&4u32.to_be_bytes()); // version
        buf[8..12].copy_from_slice(&512u32.to_be_bytes()); // header size
        buf[12..16].copy_from_slice(&1u32.to_be_bytes()); // flags
        buf[32..40].copy_from_slice(&1_048_576u64.to_be_bytes()); // data fork length
        buf[56..60].copy_from_slice(&1u32.to_be_bytes()); // segment number
        buf[60..64].copy_from_slice(&1u32.to_be_bytes()); // segment count
        buf[80..84].copy_from_slice(&22u32.to_be_bytes()); // data checksum: SHA-256
        buf[216..224].copy_from_slice(&plist_off.to_be_bytes());
        buf[224..232].copy_from_slice(&plist_len.to_be_bytes());
        buf[352..356].copy_from_slice(&22u32.to_be_bytes()); // master checksum: SHA-256
        buf[488..492].copy_from_slice(&variant.to_be_bytes());
        buf[492..500].copy_from_slice(&sectors.to_be_bytes());
        buf
    }

    #[test]
    fn parses_minimal_device_trailer() {
        let buf = build_trailer(1, 2048, 1024, 4096);
        let meta = parse(&buf).expect("valid trailer");
        assert_eq!(meta.udif_version, 4);
        assert_eq!(meta.variant, DmgVariant::Device);
        assert_eq!(meta.total_size_bytes, 2048 * 512);
        assert_eq!(meta.data_fork_length, 1_048_576);
        assert!(meta.plist_present);
        assert_eq!(meta.plist_length, 4096);
        assert_eq!(meta.segment_number, 1);
        assert_eq!(meta.segment_count, 1);
        assert_eq!(meta.data_checksum_type, DmgChecksumKind::Sha256);
        assert_eq!(meta.master_checksum_type, DmgChecksumKind::Sha256);
    }

    #[test]
    fn unrecognised_variant_preserved() {
        let buf = build_trailer(99, 512, 0, 0);
        let meta = parse(&buf).unwrap();
        assert_eq!(meta.variant, DmgVariant::Other(99));
        assert!(!meta.plist_present);
    }

    #[test]
    fn rejects_missing_magic() {
        let mut buf = build_trailer(1, 1, 0, 0);
        buf[0..4].copy_from_slice(b"junk");
        assert!(parse(&buf).is_none());
    }

    #[test]
    fn rejects_wrong_length() {
        let buf = vec![0u8; 256];
        assert!(parse(&buf).is_none());
    }

    #[test]
    fn checksum_kind_known_values() {
        assert_eq!(checksum_kind(0), DmgChecksumKind::None);
        assert_eq!(checksum_kind(2), DmgChecksumKind::Md5);
        assert_eq!(checksum_kind(17), DmgChecksumKind::Crc32);
        assert_eq!(checksum_kind(19), DmgChecksumKind::Sha1);
        assert_eq!(checksum_kind(22), DmgChecksumKind::Sha256);
        assert_eq!(checksum_kind(999), DmgChecksumKind::Other(999));
    }
}
