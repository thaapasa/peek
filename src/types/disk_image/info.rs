//! Disk-image info shape: per-format metadata payload plus
//! parse-error surface. Each variant of [`DiskImageMeta`] holds the
//! parser-specific structure (ISO 9660 PVD fields, UDIF trailer, MBR
//! partition table). The `error` field on [`DiskImageInfo`] is the
//! user-facing reason when no payload could be produced.

pub struct DiskImageInfo {
    pub format_name: &'static str,
    /// Per-format payload. `None` when parsing failed; the `error`
    /// field then carries a user-facing reason.
    pub meta: Option<DiskImageMeta>,
    /// Set when descriptor / trailer parsing failed (corrupt or
    /// truncated image). Surfaced in place of normal volume rows.
    pub error: Option<String>,
}

/// Format-specific disk-image metadata. Each variant owns the parsed
/// shape its parser produces; the renderer picks the matching block.
pub enum DiskImageMeta {
    Iso(IsoVolumeMeta),
    Dmg(DmgMeta),
    Raw(RawImageMeta),
}

/// Generic raw disk image — anything that isn't ISO/DMG and lands at
/// the `.img` / `.bin` / `.dd` extension. The MBR partition table is
/// the one structure we parse from the front of the file; without it
/// (or without a recognisable partition layout) the info section
/// falls back to a generic "raw image" label plus the file size.
pub struct RawImageMeta {
    /// MBR partition table contents when the boot sector signature
    /// (`0x55 0xAA` at offset 510) matches; `None` for files that
    /// don't carry an MBR (e.g. raw filesystem dumps, GPT-only
    /// images, or anything else).
    pub mbr: Option<MbrTable>,
}

/// MBR (Master Boot Record) partition table — the four 16-byte
/// entries at offset 446..510 of the boot sector. Extended partition
/// chains are not followed; an `0x05` / `0x0F` entry surfaces as-is
/// with the user left to inspect further.
pub struct MbrTable {
    pub partitions: Vec<MbrPartition>,
}

pub struct MbrPartition {
    pub bootable: bool,
    /// One-byte partition type code. The renderer maps a few common
    /// values (FAT, Linux, swap, …) to friendly names; anything else
    /// shows as the hex code.
    pub type_code: u8,
    /// Starting LBA from the partition entry (little-endian u32 at
    /// offset +8).
    pub start_lba: u32,
    /// Sector count from the partition entry (offset +12).
    pub sectors: u32,
}

/// ISO 9660 Primary Volume Descriptor metadata (PVD-only — no directory
/// walk). Trailing-space-padded text fields arrive trimmed; all-zero or
/// blank fields land as `None`.
pub struct IsoVolumeMeta {
    pub system_id: Option<String>,
    pub volume_label: Option<String>,
    pub volume_set_id: Option<String>,
    pub publisher: Option<String>,
    pub data_preparer: Option<String>,
    pub application: Option<String>,
    pub block_size: u32,
    pub block_count: u32,
    pub creation: Option<IsoDateTime>,
    pub modification: Option<IsoDateTime>,
    pub expiration: Option<IsoDateTime>,
    pub effective: Option<IsoDateTime>,
    pub joliet: bool,
    pub el_torito: bool,
    pub el_torito_id: Option<String>,
}

/// ISO 9660 ASCII timestamp (`YYYYMMDDHHMMSSHH±qq`). Hundredths of a
/// second are dropped on display; offset is held as 15-minute quarters
/// from GMT (range −48..=+52). All-zero source means "unset" and is
/// represented by `Option::None` rather than this struct.
pub struct IsoDateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// Quarter-hour offset from GMT.
    pub gmt_offset_quarters: i8,
}

/// UDIF (Apple Disk Image) trailer fields. The 512-byte trailer at the
/// end of every flat DMG carries the structural information; partition
/// payload sits in an embedded plist that's not parsed at this level.
pub struct DmgMeta {
    pub udif_version: u32,
    pub flags: u32,
    pub variant: DmgVariant,
    /// Sector count from the trailer × 512. Logical (uncompressed) size.
    pub total_size_bytes: u64,
    pub data_fork_length: u64,
    /// Whether the trailer references an embedded XML plist (typical;
    /// the plist holds the partition map / blkx tables).
    pub plist_present: bool,
    pub plist_length: u64,
    pub segment_number: u32,
    pub segment_count: u32,
    pub data_checksum_type: DmgChecksumKind,
    pub master_checksum_type: DmgChecksumKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmgVariant {
    /// Whole-device image (variant 1) — the common case.
    Device,
    /// Single-partition image (variant 2).
    Partition,
    /// Mounted-system image (variant 3).
    MountedSystem,
    /// Anything else — surface the raw value so the user can spot
    /// unfamiliar variants without us silently lying.
    Other(u32),
}

/// Apple's documented checksum-type tags used by both data and master
/// checksum fields in the UDIF trailer. Values outside the known set
/// surface as `Other(_)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmgChecksumKind {
    None,
    Crc32,
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Other(u32),
}
