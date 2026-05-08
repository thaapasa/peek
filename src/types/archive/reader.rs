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
            // Phase 1: an archive over a range view (e.g. an archive
            // embedded in an ISO entry, encountered via recursive peek)
            // reads its bytes eagerly into memory. Phase 2 can replace
            // this with a Read+Seek adapter that translates offsets
            // lazily over the backing file.
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
    fn list_7z_finds_expected_entries() {
        let entries = list_entries(&fixture("archive.7z"), ArchiveFormat::SevenZ).unwrap();
        let stats = Stats::from_root(ArchiveFormat::SevenZ.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }
}
