//! Archive listing dispatch: maps an `ArchiveFormat` to its backend
//! and returns a generic `Vec<Entry>` tree via `types::listing`. The
//! shared `ReadSeek` helper lives here because every backend needs a
//! seekable reader over the source.

use std::io::{Cursor, Read, Seek};

use anyhow::{Context, Result};

use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;
use crate::types::listing::{Entry, FlatEntry, from_flat_paths};

/// Trait alias for the seekable readers we hand to the zip backend. tar
/// only needs `Read`, but using one helper for both keeps the call sites
/// uniform.
pub(crate) trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// Open a `Read + Seek` over the source. File-backed sources open the
/// underlying path (and seek to the range start when needed); in-memory
/// sources share their `Bytes` via cheap clone.
pub(crate) fn open_seekable(source: &InputSource) -> Result<Box<dyn ReadSeek>> {
    match source {
        InputSource::File(path) => {
            let f = std::fs::File::open(path)
                .with_context(|| format!("failed to open {}", path.display()))?;
            Ok(Box::new(f))
        }
        InputSource::Memory { bytes, .. } => Ok(Box::new(Cursor::new(bytes.clone()))),
        InputSource::FileRange { .. } => {
            // Archive over a range view (e.g. recursive peek into an
            // archive embedded in an ISO entry): read bytes eagerly.
            let buf = source.read_bytes()?;
            Ok(Box::new(Cursor::new(buf)))
        }
    }
}

/// Enumerate the archive's table of contents as a built listing tree.
pub fn list_entries(source: &InputSource, format: ArchiveFormat) -> Result<Vec<Entry>> {
    let flat = list_flat(source, format)?;
    Ok(from_flat_paths(flat))
}

fn list_flat(source: &InputSource, format: ArchiveFormat) -> Result<Vec<FlatEntry>> {
    use super::backends::{ar, cpio, sevenz, single_stream, tar, zip};
    // Single-stream codecs short-circuit `open_seekable` — they don't
    // need the bytes, just the source name to derive the entry label.
    if matches!(
        format,
        ArchiveFormat::Gz | ArchiveFormat::Bz2 | ArchiveFormat::Xz | ArchiveFormat::Zst
    ) {
        return Ok(single_stream::list(source.name(), format));
    }
    let reader = open_seekable(source)?;
    match format {
        ArchiveFormat::Zip => zip::list(reader),
        ArchiveFormat::Tar => tar::list_plain(reader),
        ArchiveFormat::TarGz => tar::list_gz(reader),
        ArchiveFormat::TarBz2 => tar::list_bz2(reader),
        ArchiveFormat::TarXz => tar::list_xz(reader),
        ArchiveFormat::TarZst => tar::list_zst(reader),
        ArchiveFormat::TarLz4 => tar::list_lz4(reader),
        ArchiveFormat::SevenZ => sevenz::list(reader),
        ArchiveFormat::Ar => ar::list(reader),
        ArchiveFormat::Cpio => cpio::list_plain(reader),
        ArchiveFormat::CpioGz => cpio::list_gz(reader),
        ArchiveFormat::Gz | ArchiveFormat::Bz2 | ArchiveFormat::Xz | ArchiveFormat::Zst => {
            unreachable!("single-stream formats handled above")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::listing::Stats;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("test-data");
        p.push(name);
        InputSource::File(p)
    }

    /// All formats list the same 14 files. Directory counts vary by
    /// format (zip omits the archive root by convention; tar/7z
    /// include it as `./`). Total uncompressed size is consistent.
    #[test]
    fn list_zip_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.zip"), ArchiveFormat::Zip).unwrap();
        let stats = Stats::from_root(ArchiveFormat::Zip.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar"), ArchiveFormat::Tar).unwrap();
        let stats = Stats::from_root(ArchiveFormat::Tar.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_gz_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar.gz"), ArchiveFormat::TarGz).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarGz.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_bz2_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar.bz2"), ArchiveFormat::TarBz2).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarBz2.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_xz_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar.xz"), ArchiveFormat::TarXz).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarXz.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_zst_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar.zst"), ArchiveFormat::TarZst).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarZst.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_lz4_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar.lz4"), ArchiveFormat::TarLz4).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarLz4.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_cpio_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.cpio"), ArchiveFormat::Cpio).unwrap();
        let stats = Stats::from_root(ArchiveFormat::Cpio.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_cpio_gz_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.cpio.gz"), ArchiveFormat::CpioGz).unwrap();
        let stats = Stats::from_root(ArchiveFormat::CpioGz.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn single_stream_listing_returns_one_entry_per_codec() {
        for (file, fmt, expected_name) in [
            ("single.gz", ArchiveFormat::Gz, "single"),
            ("single.bz2", ArchiveFormat::Bz2, "single"),
            ("single.xz", ArchiveFormat::Xz, "single"),
            ("single.zst", ArchiveFormat::Zst, "single"),
        ] {
            let entries = list_entries(&fixture(file), fmt).unwrap();
            assert_eq!(entries.len(), 1, "{file}: expected one entry");
            assert_eq!(entries[0].name, expected_name, "{file}: stripped name");
            assert!(!entries[0].is_dir(), "{file}: not a directory");
        }
    }

    #[test]
    fn list_ar_finds_deb_members() {
        // hello.deb is a 3-member ar archive: debian-binary,
        // control.tar.gz, data.tar.gz.
        let entries = list_entries(&fixture("hello.deb"), ArchiveFormat::Ar).unwrap();
        let stats = Stats::from_root(ArchiveFormat::Ar.label(), &entries);
        assert_eq!(stats.file_count, 3);
        assert_eq!(stats.dir_count, 0);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"debian-binary"));
        assert!(names.contains(&"control.tar.gz"));
        assert!(names.contains(&"data.tar.gz"));
    }

    #[test]
    fn list_7z_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.7z"), ArchiveFormat::SevenZ).unwrap();
        let stats = Stats::from_root(ArchiveFormat::SevenZ.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    /// Empty `.tar`: zero-byte input must list as an empty TOC, not
    /// hang. Reproduces the "archive.tar size 0" case where a disk
    /// image surfaced a 0-byte tar entry and descending into it
    /// reached the listing path with empty bytes.
    #[test]
    fn empty_tar_lists_as_empty_toc() {
        let src = InputSource::memory(bytes::Bytes::new(), "empty.tar");
        let entries = list_entries(&src, ArchiveFormat::Tar).unwrap();
        assert!(entries.is_empty());
    }

    /// Compressed-tar listings against zero-byte input must finish —
    /// either with an empty TOC or a clean error. The decoders fail
    /// at the magic-byte check; the archive backend surfaces that as
    /// an `Err`. Asserting *completion* (not the specific result)
    /// is the contract that prevents the hang.
    #[test]
    fn empty_compressed_tar_listings_terminate() {
        for fmt in [
            ArchiveFormat::TarGz,
            ArchiveFormat::TarBz2,
            ArchiveFormat::TarXz,
            ArchiveFormat::TarZst,
        ] {
            let src = InputSource::memory(bytes::Bytes::new(), "empty.tar.x");
            let _ = list_entries(&src, fmt);
        }
    }

    /// Single-stream `.gz` / `.bz2` / `.xz` / `.zst` listings derive
    /// the entry name from the source filename without touching the
    /// stream — empty input must still produce the lone synthetic
    /// entry instead of hanging on a decode of zero bytes.
    #[test]
    fn empty_single_stream_listings_produce_one_entry() {
        for (name, fmt) in [
            ("empty.gz", ArchiveFormat::Gz),
            ("empty.bz2", ArchiveFormat::Bz2),
            ("empty.xz", ArchiveFormat::Xz),
            ("empty.zst", ArchiveFormat::Zst),
        ] {
            let src = InputSource::memory(bytes::Bytes::new(), name);
            let entries = list_entries(&src, fmt).unwrap();
            assert_eq!(entries.len(), 1, "{name}: one synthetic entry");
        }
    }
}
