//! Parse the MBR (Master Boot Record) partition table at the start
//! of a raw disk image. Only the legacy 4-entry table at offset
//! 446..510 plus the 0x55 0xAA boot signature are inspected — extended
//! partitions, GPT, and protective MBR variants aren't followed.

use crate::info::{MbrPartition, MbrTable};

/// Bytes pulled from the start of the image. 512 bytes (one sector)
/// holds the entire boot sector including the partition table and
/// signature.
pub(super) const BOOT_SECTOR_BYTES: usize = 512;
const TABLE_OFFSET: usize = 446;
const SIG_OFFSET: usize = 510;
const ENTRY_SIZE: usize = 16;
const ENTRY_COUNT: usize = 4;

/// Parse the MBR partition table from a 512-byte boot sector. Returns
/// `None` when the magic signature is absent or the buffer is too
/// short — the caller falls back to the "raw image" label with no
/// partition rows.
pub(super) fn parse(boot_sector: &[u8]) -> Option<MbrTable> {
    if boot_sector.len() < BOOT_SECTOR_BYTES {
        return None;
    }
    if boot_sector[SIG_OFFSET] != 0x55 || boot_sector[SIG_OFFSET + 1] != 0xAA {
        return None;
    }
    let mut partitions = Vec::with_capacity(ENTRY_COUNT);
    for i in 0..ENTRY_COUNT {
        let off = TABLE_OFFSET + i * ENTRY_SIZE;
        let entry = &boot_sector[off..off + ENTRY_SIZE];
        let type_code = entry[4];
        if type_code == 0 {
            // Empty entry — no partition slot used.
            continue;
        }
        let bootable = entry[0] == 0x80;
        let start_lba = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
        let sectors = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]);
        partitions.push(MbrPartition {
            bootable,
            type_code,
            start_lba,
            sectors,
        });
    }
    if partitions.is_empty() {
        return None;
    }
    Some(MbrTable { partitions })
}

/// Friendly name for a few common MBR partition type codes. Anything
/// not in the table renders as `0xXX` so the user can look it up.
pub fn type_label(code: u8) -> Option<&'static str> {
    Some(match code {
        0x01 => "FAT12",
        0x04 => "FAT16 (<32M)",
        0x05 => "DOS extended",
        0x06 => "FAT16",
        0x07 => "NTFS / exFAT / HPFS",
        0x0B => "FAT32 (CHS)",
        0x0C => "FAT32 (LBA)",
        0x0E => "FAT16 (LBA)",
        0x0F => "Win extended (LBA)",
        0x11 => "Hidden FAT12",
        0x14 => "Hidden FAT16 (<32M)",
        0x16 => "Hidden FAT16",
        0x17 => "Hidden NTFS",
        0x1B => "Hidden FAT32 (CHS)",
        0x1C => "Hidden FAT32 (LBA)",
        0x27 => "Windows recovery",
        0x82 => "Linux swap / Solaris",
        0x83 => "Linux",
        0x85 => "Linux extended",
        0x8E => "Linux LVM",
        0xA5 => "FreeBSD",
        0xA6 => "OpenBSD",
        0xA8 => "Darwin UFS",
        0xA9 => "NetBSD",
        0xAB => "Darwin boot",
        0xAF => "HFS / HFS+",
        0xEE => "GPT protective",
        0xEF => "EFI system",
        0xFD => "Linux RAID",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_boot_sector() -> Vec<u8> {
        let mut buf = vec![0u8; BOOT_SECTOR_BYTES];
        // First entry: bootable Linux, LBA 2048, 100000 sectors.
        let off = TABLE_OFFSET;
        buf[off] = 0x80;
        buf[off + 4] = 0x83;
        buf[off + 8..off + 12].copy_from_slice(&2048u32.to_le_bytes());
        buf[off + 12..off + 16].copy_from_slice(&100_000u32.to_le_bytes());
        // Boot signature
        buf[SIG_OFFSET] = 0x55;
        buf[SIG_OFFSET + 1] = 0xAA;
        buf
    }

    #[test]
    fn parses_one_entry_with_signature() {
        let buf = synth_boot_sector();
        let table = parse(&buf).expect("expected parse");
        assert_eq!(table.partitions.len(), 1);
        assert!(table.partitions[0].bootable);
        assert_eq!(table.partitions[0].type_code, 0x83);
        assert_eq!(table.partitions[0].start_lba, 2048);
        assert_eq!(table.partitions[0].sectors, 100_000);
    }

    #[test]
    fn missing_signature_returns_none() {
        let mut buf = synth_boot_sector();
        buf[SIG_OFFSET] = 0;
        assert!(parse(&buf).is_none());
    }

    #[test]
    fn empty_table_returns_none() {
        let mut buf = vec![0u8; BOOT_SECTOR_BYTES];
        buf[SIG_OFFSET] = 0x55;
        buf[SIG_OFFSET + 1] = 0xAA;
        assert!(parse(&buf).is_none());
    }
}
