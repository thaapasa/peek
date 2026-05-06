//! Shared archive data shapes plus the format-dispatching `list_entries`
//! entry point. Backends in `super::backends` decode each format; this
//! module owns the public types they produce.

use std::io::{Cursor, Read, Seek};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;

/// One entry in an archive TOC.
pub struct ArchiveEntry {
    /// Entry path as stored in the archive, normalized to forward slashes.
    pub path: String,
    /// Uncompressed size in bytes. `0` for directories and empty files.
    pub size: u64,
    /// Last-modified time, when the archive's per-entry header carries one.
    pub mtime: Option<ArchiveMtime>,
    /// Unix permission bits (e.g. `0o755`), when present.
    pub mode: Option<u32>,
    pub is_dir: bool,
}

/// Per-entry mtime. Tar carries Unix epoch seconds (UTC); zip carries
/// MS-DOS wall-clock without a timezone. Keeping the two distinct lets
/// the viewer avoid treating zip stamps as UTC and double-shifting the
/// display offset.
pub enum ArchiveMtime {
    Utc(SystemTime),
    LocalNaive {
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
    },
}

/// Aggregate stats over a TOC, used by the info view.
pub struct ArchiveStats {
    pub format_name: &'static str,
    pub entry_count: usize,
    pub file_count: usize,
    pub dir_count: usize,
    pub total_uncompressed_size: u64,
}

impl ArchiveStats {
    pub fn from_entries(format: ArchiveFormat, entries: &[ArchiveEntry]) -> Self {
        let mut file_count = 0;
        let mut dir_count = 0;
        let mut total = 0u64;
        for e in entries {
            if e.is_dir {
                dir_count += 1;
            } else {
                file_count += 1;
                total = total.saturating_add(e.size);
            }
        }
        Self {
            format_name: format.label(),
            entry_count: entries.len(),
            file_count,
            dir_count,
            total_uncompressed_size: total,
        }
    }
}

/// Trait alias for the seekable readers we hand to the zip backend. tar
/// only needs `Read`, but using one helper for both keeps the call sites
/// uniform.
pub(crate) trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// Open a `Read + Seek` over the source. Files re-open the underlying
/// path; stdin shares the buffered bytes via Arc → no copy.
pub(crate) fn open_seekable(source: &InputSource) -> Result<Box<dyn ReadSeek>> {
    match source {
        InputSource::File(path) => {
            let f = std::fs::File::open(path)
                .with_context(|| format!("failed to open {}", path.display()))?;
            Ok(Box::new(f))
        }
        InputSource::Stdin { data } => Ok(Box::new(Cursor::new(Arc::clone(data)))),
    }
}

/// Enumerate the archive's table of contents.
pub fn list_entries(source: &InputSource, format: ArchiveFormat) -> Result<Vec<ArchiveEntry>> {
    use super::backends::{sevenz, tar, zip};
    let reader = open_seekable(source)?;
    match format {
        ArchiveFormat::Zip => zip::list(reader),
        ArchiveFormat::Tar => tar::list_plain(reader),
        ArchiveFormat::TarGz => tar::list_gz(reader),
        ArchiveFormat::TarBz2 => tar::list_bz2(reader),
        ArchiveFormat::TarXz => tar::list_xz(reader),
        ArchiveFormat::TarZst => tar::list_zst(reader),
        ArchiveFormat::SevenZ => sevenz::list(reader),
    }
}

/// Build a `SystemTime` from Unix epoch seconds, returning `None` for
/// pre-epoch or unset (`0`) timestamps.
pub(crate) fn time_from_epoch_secs(secs: u64) -> Option<SystemTime> {
    if secs == 0 {
        return None;
    }
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("test-data");
        p.push(name);
        InputSource::File(p)
    }

    /// All three formats list the same 14 files plus directory entries.
    /// Tar / tar.gz include the archive root `./` so report 17 entries
    /// total; zip omits the root (zip stores entries with relative paths
    /// from the root by convention) so reports 16.
    #[test]
    fn list_zip_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.zip"), ArchiveFormat::Zip).unwrap();
        let stats = ArchiveStats::from_entries(ArchiveFormat::Zip, &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_uncompressed_size, 30_683);
    }

    #[test]
    fn list_tar_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar"), ArchiveFormat::Tar).unwrap();
        let stats = ArchiveStats::from_entries(ArchiveFormat::Tar, &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 3);
        assert_eq!(stats.total_uncompressed_size, 30_683);
    }

    #[test]
    fn list_tar_gz_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar.gz"), ArchiveFormat::TarGz).unwrap();
        let stats = ArchiveStats::from_entries(ArchiveFormat::TarGz, &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 3);
        assert_eq!(stats.total_uncompressed_size, 30_683);
    }

    #[test]
    fn list_tar_bz2_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar.bz2"), ArchiveFormat::TarBz2).unwrap();
        let stats = ArchiveStats::from_entries(ArchiveFormat::TarBz2, &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 3);
        assert_eq!(stats.total_uncompressed_size, 30_683);
    }

    #[test]
    fn list_tar_xz_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar.xz"), ArchiveFormat::TarXz).unwrap();
        let stats = ArchiveStats::from_entries(ArchiveFormat::TarXz, &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 3);
        assert_eq!(stats.total_uncompressed_size, 30_683);
    }

    #[test]
    fn list_tar_zst_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.tar.zst"), ArchiveFormat::TarZst).unwrap();
        let stats = ArchiveStats::from_entries(ArchiveFormat::TarZst, &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 3);
        assert_eq!(stats.total_uncompressed_size, 30_683);
    }

    #[test]
    fn list_7z_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.7z"), ArchiveFormat::SevenZ).unwrap();
        let stats = ArchiveStats::from_entries(ArchiveFormat::SevenZ, &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_uncompressed_size, 30_683);
    }
}
