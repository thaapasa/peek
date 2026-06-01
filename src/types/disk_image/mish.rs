//! UDIF "mish" (BLKX) block-table parser.
//!
//! Each blkx entry in a DMG's embedded plist (see [`super::dmg_plist`])
//! decodes to a `mish` block: a fixed 204-byte header followed by a run
//! of 40-byte chunk descriptors. The chunks describe how the partition's
//! sectors map into the data fork — by compression method, sector span,
//! and stored length — without us decompressing a single byte.
//!
//! We read only the structural fields the info view surfaces: the
//! partition's sector span and a per-chunk (kind, sector count, stored
//! length) tuple. Reconstructing the partition payload (decoding the
//! zlib / lzfse / … runs) is a separate, deferred effort.

/// `mish` magic at the head of every block table.
const SIGNATURE: &[u8; 4] = b"mish";

/// Fixed header length up to and including the 4-byte chunk count at
/// offset 200. Chunk descriptors follow immediately after.
const HEADER_LEN: usize = 204;

/// Apple's sector size for UDIF images.
const SECTOR_SIZE: u64 = 512;

/// One block-table chunk descriptor (40 bytes on disk).
const CHUNK_LEN: usize = 40;

/// Parsed block table for one partition. `runs` excludes nothing — the
/// comment / terminator markers are kept so callers can reason about the
/// raw table; [`MishTable::summary`] is the filtered view.
pub struct MishTable {
    /// Sector span of this partition (drives the logical size).
    pub sector_count: u64,
    pub runs: Vec<MishRun>,
}

pub struct MishRun {
    pub kind: RunKind,
    /// Bytes this run occupies in the data fork. Zero for zero-fill /
    /// ignore runs.
    pub stored_length: u64,
}

/// Block-chunk entry types from Apple's UDIF spec. The compression
/// variants name the codec the run's bytes are encoded with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunKind {
    /// Sparse zero region — no stored bytes.
    ZeroFill,
    /// Uncompressed copy (UDRW).
    Raw,
    /// Unallocated / free — treated as zero, no stored bytes.
    Ignore,
    Adc,
    Zlib,
    Bzip2,
    Lzfse,
    Lzma,
    /// Comment marker — carries no payload.
    Comment,
    /// Final entry of the table.
    Terminator,
    /// Anything outside the documented set; raw tag preserved.
    Other(u32),
}

impl RunKind {
    fn from_tag(raw: u32) -> RunKind {
        match raw {
            0x0000_0000 => RunKind::ZeroFill,
            0x0000_0001 => RunKind::Raw,
            0x0000_0002 => RunKind::Ignore,
            0x8000_0004 => RunKind::Adc,
            0x8000_0005 => RunKind::Zlib,
            0x8000_0006 => RunKind::Bzip2,
            0x8000_0007 => RunKind::Lzfse,
            0x8000_0008 => RunKind::Lzma,
            0x7fff_fffe => RunKind::Comment,
            0xffff_ffff => RunKind::Terminator,
            other => RunKind::Other(other),
        }
    }

    /// Marker runs carry no data — excluded from chunk counts and stored
    /// totals.
    fn is_marker(self) -> bool {
        matches!(self, RunKind::Comment | RunKind::Terminator)
    }

    /// Codec label when this run is compressed; `None` for raw / sparse /
    /// marker runs.
    fn compression_label(self) -> Option<&'static str> {
        Some(match self {
            RunKind::Adc => "ADC",
            RunKind::Zlib => "zlib",
            RunKind::Bzip2 => "bzip2",
            RunKind::Lzfse => "lzfse",
            RunKind::Lzma => "lzma",
            _ => return None,
        })
    }
}

/// Aggregated view of a block table — the Layer-2 summary surfaced in the
/// info section.
pub struct MishSummary {
    /// Logical partition size: sector span × 512.
    pub uncompressed_bytes: u64,
    /// On-disk footprint: sum of every data chunk's stored length.
    pub stored_bytes: u64,
    /// Number of data chunks (markers excluded).
    pub chunk_count: usize,
    /// Distinct compression codecs in first-seen order.
    pub methods: Vec<&'static str>,
}

impl MishTable {
    /// Parse a decoded `mish` block. Returns `None` when the buffer is
    /// too short or the signature is missing (e.g. a non-blkx plist
    /// entry whose `Data` isn't a block table).
    pub fn parse(buf: &[u8]) -> Option<MishTable> {
        if buf.len() < HEADER_LEN || &buf[0..4] != SIGNATURE {
            return None;
        }
        let sector_count = read_u64(&buf[16..24]);
        let declared = read_u32(&buf[200..204]) as usize;

        // Trust the buffer over the declared count — a corrupt count
        // can't drive an over-read.
        let available = (buf.len() - HEADER_LEN) / CHUNK_LEN;
        let n = declared.min(available);

        let mut runs = Vec::with_capacity(n);
        for i in 0..n {
            let off = HEADER_LEN + i * CHUNK_LEN;
            let kind = RunKind::from_tag(read_u32(&buf[off..off + 4]));
            let stored_length = read_u64(&buf[off + 32..off + 40]);
            runs.push(MishRun {
                kind,
                stored_length,
            });
        }

        Some(MishTable { sector_count, runs })
    }

    pub fn summary(&self) -> MishSummary {
        let mut stored_bytes = 0u64;
        let mut chunk_count = 0usize;
        let mut methods: Vec<&'static str> = Vec::new();
        for run in &self.runs {
            if run.kind.is_marker() {
                continue;
            }
            chunk_count += 1;
            stored_bytes = stored_bytes.saturating_add(run.stored_length);
            if let Some(m) = run.kind.compression_label()
                && !methods.contains(&m)
            {
                methods.push(m);
            }
        }
        MishSummary {
            uncompressed_bytes: self.sector_count.saturating_mul(SECTOR_SIZE),
            stored_bytes,
            chunk_count,
            methods,
        }
    }
}

fn read_u32(buf: &[u8]) -> u32 {
    u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
}

fn read_u64(buf: &[u8]) -> u64 {
    u64::from_be_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a mish block with the given (tag, sector_count, stored_len)
    /// chunks. Header fields beyond signature / sectors / count are left
    /// zero — the parser ignores them.
    fn build(first_sector: u64, sector_count: u64, chunks: &[(u32, u64, u64)]) -> Vec<u8> {
        let mut buf = vec![0u8; HEADER_LEN + chunks.len() * CHUNK_LEN];
        buf[0..4].copy_from_slice(SIGNATURE);
        buf[8..16].copy_from_slice(&first_sector.to_be_bytes());
        buf[16..24].copy_from_slice(&sector_count.to_be_bytes());
        buf[200..204].copy_from_slice(&(chunks.len() as u32).to_be_bytes());
        for (i, (tag, sc, stored)) in chunks.iter().enumerate() {
            let off = HEADER_LEN + i * CHUNK_LEN;
            buf[off..off + 4].copy_from_slice(&tag.to_be_bytes());
            buf[off + 16..off + 24].copy_from_slice(&sc.to_be_bytes());
            buf[off + 32..off + 40].copy_from_slice(&stored.to_be_bytes());
        }
        buf
    }

    #[test]
    fn rejects_short_or_unsigned() {
        assert!(MishTable::parse(&[]).is_none());
        assert!(MishTable::parse(&vec![0u8; HEADER_LEN]).is_none());
        let mut buf = build(0, 1, &[]);
        buf[0..4].copy_from_slice(b"junk");
        assert!(MishTable::parse(&buf).is_none());
    }

    #[test]
    fn parses_sectors_and_chunks() {
        // 2048 sectors = 1 MiB logical. One lzfse chunk storing 4 KiB,
        // plus a terminator that must not count.
        let buf = build(34, 2048, &[(0x8000_0007, 2048, 4096), (0xffff_ffff, 0, 0)]);
        let table = MishTable::parse(&buf).expect("valid mish");
        assert_eq!(table.sector_count, 2048);
        assert_eq!(table.runs.len(), 2);

        let s = table.summary();
        assert_eq!(s.uncompressed_bytes, 2048 * 512);
        assert_eq!(s.stored_bytes, 4096);
        assert_eq!(s.chunk_count, 1, "terminator excluded");
        assert_eq!(s.methods, vec!["lzfse"]);
    }

    #[test]
    fn zero_fill_is_sparse() {
        let buf = build(0, 8, &[(0x0000_0000, 8, 0), (0xffff_ffff, 0, 0)]);
        let s = MishTable::parse(&buf).unwrap().summary();
        assert_eq!(s.stored_bytes, 0);
        assert!(s.methods.is_empty());
        assert_eq!(s.chunk_count, 1);
    }

    #[test]
    fn distinct_methods_preserved_in_order() {
        let buf = build(
            0,
            100,
            &[
                (0x8000_0005, 10, 100), // zlib
                (0x8000_0007, 10, 100), // lzfse
                (0x8000_0005, 10, 100), // zlib again — not duplicated
                (0x0000_0001, 10, 100), // raw — no method
            ],
        );
        let s = MishTable::parse(&buf).unwrap().summary();
        assert_eq!(s.methods, vec!["zlib", "lzfse"]);
        assert_eq!(s.chunk_count, 4);
        assert_eq!(s.stored_bytes, 400);
    }

    #[test]
    fn declared_count_clamped_to_buffer() {
        let mut buf = build(0, 1, &[(0x0000_0001, 1, 10)]);
        // Lie: claim 999 chunks. Parser must clamp to the one present.
        buf[200..204].copy_from_slice(&999u32.to_be_bytes());
        let table = MishTable::parse(&buf).unwrap();
        assert_eq!(table.runs.len(), 1);
    }
}
