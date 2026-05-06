//! ISO 9660 Primary Volume Descriptor parser (header-only).
//!
//! Reads the Volume Descriptor Set starting at sector 16 (byte offset
//! 32768). Each descriptor is a 2048-byte record. We pull volume
//! metadata from the Primary VD (type 1) and scan a bounded number of
//! later descriptors for Joliet (Supplementary VD with the right
//! escape sequence) and El Torito (Boot Record). The terminator
//! descriptor (type 255) ends the walk.
//!
//! No directory walk — Rock Ridge presence isn't surfaced here because
//! it lives in SUSP fields inside the root directory record and would
//! need an extra read+parse pass.

use crate::info::{IsoDateTime, IsoVolumeMeta};

/// First sector containing the Volume Descriptor Set.
pub const PVD_OFFSET: u64 = 32768;

/// Bytes per logical sector in the descriptor area. ISO 9660 fixes
/// this at 2048; the per-volume `block_size` field can differ but
/// doesn't change the descriptor layout.
const SECTOR_SIZE: usize = 2048;

/// Cap on descriptors walked when scanning for Joliet / El Torito.
/// Real images carry a handful (PVD + maybe SVD + boot + terminator);
/// the cap stops a corrupt image from looping unbounded.
const MAX_DESCRIPTORS: usize = 16;

const STANDARD_ID: &[u8; 5] = b"CD001";

const TYPE_BOOT_RECORD: u8 = 0;
const TYPE_PRIMARY: u8 = 1;
const TYPE_SUPPLEMENTARY: u8 = 2;
const TYPE_TERMINATOR: u8 = 255;

/// Parse the descriptor area (one or more 2048-byte sectors). Returns
/// `None` when the leading sector isn't a valid Primary Volume
/// Descriptor; callers should surface this as "not an ISO 9660 image".
pub fn parse(data: &[u8]) -> Option<IsoVolumeMeta> {
    let pvd = sector(data, 0)?;
    if pvd[0] != TYPE_PRIMARY || &pvd[1..6] != STANDARD_ID {
        return None;
    }

    let mut meta = IsoVolumeMeta {
        system_id: trim_text(&pvd[8..40]),
        volume_label: trim_text(&pvd[40..72]),
        block_count: read_both_endian_u32(&pvd[80..88]),
        volume_set_id: trim_text(&pvd[190..318]),
        publisher: trim_text(&pvd[318..446]),
        data_preparer: trim_text(&pvd[446..574]),
        application: trim_text(&pvd[574..702]),
        block_size: read_both_endian_u16(&pvd[128..132]) as u32,
        creation: parse_datetime(&pvd[813..830]),
        modification: parse_datetime(&pvd[830..847]),
        expiration: parse_datetime(&pvd[847..864]),
        effective: parse_datetime(&pvd[864..881]),
        joliet: false,
        el_torito: false,
        el_torito_id: None,
    };

    for idx in 1..MAX_DESCRIPTORS {
        let Some(s) = sector(data, idx) else { break };
        if &s[1..6] != STANDARD_ID {
            break;
        }
        match s[0] {
            TYPE_TERMINATOR => break,
            TYPE_SUPPLEMENTARY if is_joliet_escape(&s[88..120]) => {
                meta.joliet = true;
            }
            TYPE_BOOT_RECORD if &s[7..30] == b"EL TORITO SPECIFICATION" => {
                meta.el_torito = true;
                meta.el_torito_id = trim_text(&s[39..71]);
            }
            _ => {}
        }
    }

    Some(meta)
}

fn sector(data: &[u8], idx: usize) -> Option<&[u8]> {
    let start = idx.checked_mul(SECTOR_SIZE)?;
    let end = start.checked_add(SECTOR_SIZE)?;
    if end > data.len() {
        return None;
    }
    Some(&data[start..end])
}

/// Trim ISO 9660 text fields. Strings are space-padded; some images
/// also pad with NULs. An all-blank field becomes `None`.
fn trim_text(buf: &[u8]) -> Option<String> {
    let trimmed: Vec<u8> = buf
        .iter()
        .copied()
        .rev()
        .skip_while(|b| *b == b' ' || *b == 0)
        .collect::<Vec<u8>>()
        .into_iter()
        .rev()
        .collect();
    if trimmed.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&trimmed).into_owned())
}

/// Both-endian 32-bit field (4 bytes LE followed by 4 bytes BE). We
/// trust the LE half; the BE half is redundant by spec.
fn read_both_endian_u32(buf: &[u8]) -> u32 {
    u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}

/// Both-endian 16-bit field (2 bytes LE + 2 bytes BE).
fn read_both_endian_u16(buf: &[u8]) -> u16 {
    u16::from_le_bytes([buf[0], buf[1]])
}

/// Joliet announces UCS-2 names via one of three escape sequences
/// in the Supplementary VD's escape area (`%/@`, `%/C`, `%/E` for
/// levels 1/2/3). Anything else in this slot is some other SVD
/// extension (rare).
fn is_joliet_escape(buf: &[u8]) -> bool {
    buf.windows(3)
        .any(|w| matches!(w, [0x25, 0x2F, 0x40 | 0x43 | 0x45]))
}

/// 17-byte ASCII PVD timestamp. `YYYYMMDDHHMMSSHH±qq` with the last
/// byte a signed quarter-hour offset from GMT. An all-zero record
/// (year `"0000"`, etc.) means "unset".
fn parse_datetime(buf: &[u8]) -> Option<IsoDateTime> {
    if buf.len() < 17 {
        return None;
    }
    let year = parse_ascii_uint(&buf[0..4])? as u16;
    let month = parse_ascii_uint(&buf[4..6])? as u8;
    let day = parse_ascii_uint(&buf[6..8])? as u8;
    let hour = parse_ascii_uint(&buf[8..10])? as u8;
    let minute = parse_ascii_uint(&buf[10..12])? as u8;
    let second = parse_ascii_uint(&buf[12..14])? as u8;
    let gmt_offset_quarters = buf[16] as i8;
    if year == 0 && month == 0 && day == 0 && hour == 0 && minute == 0 && second == 0 {
        return None;
    }
    Some(IsoDateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        gmt_offset_quarters,
    })
}

/// Parse an ASCII decimal field. Returns `None` when any byte isn't a
/// digit or a space (some writers space-pad partially-set fields).
fn parse_ascii_uint(buf: &[u8]) -> Option<u32> {
    let mut n: u32 = 0;
    for b in buf {
        match b {
            b' ' => continue,
            b'0'..=b'9' => n = n.checked_mul(10)?.checked_add((b - b'0') as u32)?,
            _ => return None,
        }
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid PVD sector for tests. Only the fields the
    /// parser inspects are populated; the rest stay zero.
    fn build_pvd(volume_label: &str, publisher: &str) -> Vec<u8> {
        let mut buf = vec![0u8; SECTOR_SIZE];
        buf[0] = TYPE_PRIMARY;
        buf[1..6].copy_from_slice(STANDARD_ID);
        // System ID 8..40
        let sysid = b"LINUX";
        buf[8..8 + sysid.len()].copy_from_slice(sysid);
        for b in &mut buf[8 + sysid.len()..40] {
            *b = b' ';
        }
        // Volume label 40..72 (D-chars, space-padded)
        for b in &mut buf[40..72] {
            *b = b' ';
        }
        let v = volume_label.as_bytes();
        buf[40..40 + v.len()].copy_from_slice(v);
        // Block count 80..88 (both-endian u32; LE half)
        let blocks: u32 = 100;
        buf[80..84].copy_from_slice(&blocks.to_le_bytes());
        buf[84..88].copy_from_slice(&blocks.to_be_bytes());
        // Block size 128..132 (both-endian u16)
        let bsz: u16 = 2048;
        buf[128..130].copy_from_slice(&bsz.to_le_bytes());
        buf[130..132].copy_from_slice(&bsz.to_be_bytes());
        // Publisher 318..446
        for b in &mut buf[318..446] {
            *b = b' ';
        }
        let p = publisher.as_bytes();
        buf[318..318 + p.len()].copy_from_slice(p);
        // Creation timestamp 813..830 — 2025-01-15 14:30:00, GMT+0
        buf[813..830].copy_from_slice(b"2025011514300000\x00");
        // Modification 830..847 — unset (all '0' digits + zero offset).
        buf[830..847].copy_from_slice(b"0000000000000000\x00");
        // Expiration 847..864 — unset.
        buf[847..864].copy_from_slice(b"0000000000000000\x00");
        // Effective 864..881 — unset.
        buf[864..881].copy_from_slice(b"0000000000000000\x00");
        buf
    }

    fn build_terminator() -> Vec<u8> {
        let mut buf = vec![0u8; SECTOR_SIZE];
        buf[0] = TYPE_TERMINATOR;
        buf[1..6].copy_from_slice(STANDARD_ID);
        buf
    }

    fn build_joliet() -> Vec<u8> {
        let mut buf = vec![0u8; SECTOR_SIZE];
        buf[0] = TYPE_SUPPLEMENTARY;
        buf[1..6].copy_from_slice(STANDARD_ID);
        // Escape sequence at 88: %/E (Joliet level 3)
        buf[88] = 0x25;
        buf[89] = 0x2F;
        buf[90] = 0x45;
        buf
    }

    fn build_el_torito() -> Vec<u8> {
        let mut buf = vec![0u8; SECTOR_SIZE];
        buf[0] = TYPE_BOOT_RECORD;
        buf[1..6].copy_from_slice(STANDARD_ID);
        buf[7..30].copy_from_slice(b"EL TORITO SPECIFICATION");
        buf
    }

    #[test]
    fn parses_minimal_pvd() {
        let mut data = build_pvd("PEEK_TEST", "TUUKKA");
        data.extend_from_slice(&build_terminator());
        let meta = parse(&data).expect("valid PVD");
        assert_eq!(meta.volume_label.as_deref(), Some("PEEK_TEST"));
        assert_eq!(meta.publisher.as_deref(), Some("TUUKKA"));
        assert_eq!(meta.system_id.as_deref(), Some("LINUX"));
        assert_eq!(meta.block_size, 2048);
        assert_eq!(meta.block_count, 100);
        let creation = meta.creation.expect("creation set");
        assert_eq!(creation.year, 2025);
        assert_eq!(creation.month, 1);
        assert_eq!(creation.day, 15);
        assert_eq!(creation.hour, 14);
        assert_eq!(creation.minute, 30);
        assert!(meta.modification.is_none());
        assert!(!meta.joliet);
        assert!(!meta.el_torito);
    }

    #[test]
    fn detects_joliet_and_el_torito() {
        let mut data = build_pvd("HYBRID", "PEEK");
        data.extend_from_slice(&build_joliet());
        data.extend_from_slice(&build_el_torito());
        data.extend_from_slice(&build_terminator());
        let meta = parse(&data).expect("valid PVD");
        assert!(meta.joliet);
        assert!(meta.el_torito);
    }

    #[test]
    fn rejects_non_iso_data() {
        let buf = vec![0u8; SECTOR_SIZE];
        assert!(parse(&buf).is_none());
    }

    #[test]
    fn rejects_truncated_descriptor_area() {
        let buf = vec![0u8; 1024];
        assert!(parse(&buf).is_none());
    }

    #[test]
    fn unset_creation_timestamp_is_none() {
        let mut buf = build_pvd("X", "Y");
        buf[813..830].copy_from_slice(b"0000000000000000\x00");
        let mut data = buf;
        data.extend_from_slice(&build_terminator());
        let meta = parse(&data).unwrap();
        assert!(meta.creation.is_none());
    }
}
