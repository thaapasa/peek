//! Archive listing dispatch: maps an `ArchiveFormat` to its backend
//! and returns a generic `Vec<Entry>` tree via `viewer::listing`. The
//! `ReadSeek` helper (re-exported from `peek-io`) is re-surfaced here
//! because every backend needs a seekable reader over the source, and the
//! cap-gated `open_zip` / `read_zip_entry` pair because every zip-backed
//! render path (DOCX, ODT, EPUB, CBZ, spreadsheet props) needs the same
//! zip-bomb gate.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::Bytes;
use tempfile::NamedTempFile;
use zip::ZipArchive;

use super::backends::CappedList;
use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;
use crate::viewer::listing::{Entry, from_flat_paths};
use crate::viewer::modes::{RENDER_MAX_BYTES, ensure_under_render_cap};

/// The seekable-reader trait the backends hand to the zip layer. tar only
/// needs `Read`, but using one helper for both keeps the call sites
/// uniform. Re-exported from `peek-io` rather than redefined — it is the
/// same `Read + Seek` alias the csv reader uses, and one definition keeps
/// the two call sites from drifting.
pub(crate) use crate::input::stream::ReadSeek;

/// Open a `Read + Seek` over the source. File-backed sources open the
/// underlying path (and seek to the range start when needed); in-memory
/// sources share their `Bytes` via cheap clone.
pub(crate) fn open_seekable(source: &InputSource) -> Result<Box<dyn ReadSeek>> {
    match source {
        InputSource::File(path) => {
            let f =
                File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
            Ok(Box::new(f))
        }
        InputSource::Memory { bytes, .. } => Ok(Box::new(Cursor::new(bytes.clone()))),
        InputSource::FileRange {
            base,
            offset,
            len,
            guard,
            ..
        } => {
            // Archive over a range view (recursive peek into an archive
            // carved as a zero-copy `FileRange`): expose the range as a
            // seekable window over the backing file — no eager read.
            Ok(Box::new(RangeReadSeek::open(
                base,
                *offset,
                *len,
                guard.clone(),
            )?))
        }
        InputSource::TempFile { file, .. } => {
            let f = File::open(file.path())
                .with_context(|| format!("failed to open tempfile {}", file.path().display()))?;
            Ok(Box::new(f))
        }
    }
}

/// Open a ZIP archive over the source. `label` names the container kind
/// ("EPUB", "DOCX", …) in error messages.
pub(crate) fn open_zip(source: &InputSource, label: &str) -> Result<ZipArchive<Box<dyn ReadSeek>>> {
    let reader =
        open_seekable(source).with_context(|| format!("failed to open {label} container"))?;
    ZipArchive::new(reader).with_context(|| format!("failed to read {label} archive"))
}

/// Read one entry's bytes out of an open ZIP. Gates on the *uncompressed*
/// entry size, not the container — a small container can declare a
/// multi-GB entry (zip bomb). Above the render cap, refuse before the
/// whole-entry allocation; the rendered view is dropped and the container
/// TOC + hex view stand in.
pub(crate) fn read_zip_entry(
    zip: &mut ZipArchive<Box<dyn ReadSeek>>,
    path: &str,
    label: &str,
) -> Result<Bytes> {
    let file = zip
        .by_name(path)
        .with_context(|| format!("{label} entry {path:?} not found"))?;
    let declared = file.size();
    ensure_under_render_cap(declared, path)?;
    // The declared size is attacker-controlled metadata and the deflate
    // decoder doesn't stop at it — bound the actual decompressed read too,
    // so an entry that lies *small* can't expand past the cap either.
    let mut buf = Vec::with_capacity(declared as usize);
    file.take(RENDER_MAX_BYTES + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > RENDER_MAX_BYTES {
        anyhow::bail!(
            "{path} decompresses past the {} MB render cap (declared {declared} B)",
            RENDER_MAX_BYTES / (1024 * 1024)
        );
    }
    Ok(Bytes::from(buf))
}

/// [`read_zip_entry`] decoded as UTF-8, for the XML-payload callers.
pub(crate) fn read_zip_entry_str(
    zip: &mut ZipArchive<Box<dyn ReadSeek>>,
    path: &str,
    label: &str,
) -> Result<String> {
    let bytes = read_zip_entry(zip, path, label)?;
    String::from_utf8(bytes.into()).with_context(|| format!("{label} entry {path:?} is not UTF-8"))
}

/// `Read + Seek` window over `[start, start+len)` of a backing file.
/// Lets archive backends walk a `FileRange` source (an entry carved as a
/// zero-copy view) without buffering the range into memory. Holds the
/// `Arc<NamedTempFile>` guard when the backing file is a spool, so the
/// range outlives the source it was opened from.
struct RangeReadSeek {
    file: File,
    start: u64,
    len: u64,
    /// Cursor position within the window, in `[0, len]`.
    pos: u64,
    _guard: Option<Arc<NamedTempFile>>,
}

impl RangeReadSeek {
    fn open(base: &Path, offset: u64, len: u64, guard: Option<Arc<NamedTempFile>>) -> Result<Self> {
        let mut file =
            File::open(base).with_context(|| format!("failed to open {}", base.display()))?;
        file.seek(SeekFrom::Start(offset))
            .with_context(|| format!("failed to seek in {}", base.display()))?;
        Ok(Self {
            file,
            start: offset,
            len,
            pos: 0,
            _guard: guard,
        })
    }
}

impl Read for RangeReadSeek {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.len - self.pos;
        if remaining == 0 {
            return Ok(0);
        }
        let want = (buf.len() as u64).min(remaining) as usize;
        let n = self.file.read(&mut buf[..want])?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for RangeReadSeek {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::End(n) => self.len as i64 + n,
            SeekFrom::Current(n) => self.pos as i64 + n,
        };
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start of range",
            ));
        }
        let target = (target as u64).min(self.len);
        self.file.seek(SeekFrom::Start(self.start + target))?;
        self.pos = target;
        Ok(self.pos)
    }
}

/// Enumerate the archive's table of contents as a built listing tree.
/// The bool is `true` when the listing hit
/// [`MAX_ENTRIES`](super::backends::MAX_ENTRIES) and was truncated, so
/// callers can surface a note rather than under-report silently.
pub fn list_entries(source: &InputSource, format: ArchiveFormat) -> Result<(Vec<Entry>, bool)> {
    let capped = list_flat(source, format)?;
    Ok((from_flat_paths(capped.entries), capped.truncated))
}

fn list_flat(source: &InputSource, format: ArchiveFormat) -> Result<CappedList> {
    use super::backends::{ar, cpio, sevenz, tar, zip};
    let reader = open_seekable(source)?;
    match format {
        ArchiveFormat::Zip => zip::list(reader),
        ArchiveFormat::Tar => tar::list_plain(reader),
        ArchiveFormat::TarGz => tar::list_gz(reader),
        ArchiveFormat::TarBz2 => tar::list_bz2(reader),
        ArchiveFormat::TarXz => tar::list_xz(reader),
        ArchiveFormat::TarZst => tar::list_zst(reader),
        ArchiveFormat::TarLz4 => tar::list_lz4(reader),
        ArchiveFormat::TarBr => tar::list_br(reader),
        ArchiveFormat::SevenZ => sevenz::list(reader),
        ArchiveFormat::Ar => ar::list(reader),
        ArchiveFormat::Cpio => cpio::list_plain(reader),
        ArchiveFormat::CpioGz => cpio::list_gz(reader),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::listing::Stats;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        p.push("test-data");
        p.push(name);
        InputSource::File(p)
    }

    /// `open_seekable` over a `FileRange` must expose a windowed `Read +
    /// Seek` over the backing file (via `RangeReadSeek`) rather than
    /// buffering the range into memory. Exercises read clamping and all
    /// three `SeekFrom` variants.
    #[test]
    fn open_seekable_file_range_reads_and_seeks() {
        use std::io::{Read, Seek, SeekFrom, Write};
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"0123456789").unwrap();
        tmp.flush().unwrap();
        // Window = bytes 2..7 = "23456".
        let src = InputSource::File(tmp.path().to_path_buf()).subrange(2, 5, "mid");
        let mut r = open_seekable(&src).unwrap();

        let mut all = Vec::new();
        r.read_to_end(&mut all).unwrap();
        assert_eq!(all, b"23456");

        r.seek(SeekFrom::Start(1)).unwrap();
        let mut two = [0u8; 2];
        r.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"34");

        r.seek(SeekFrom::End(-1)).unwrap();
        let mut last = [0u8; 1];
        r.read_exact(&mut last).unwrap();
        assert_eq!(&last, b"6");

        // At window EOF, reads return 0 even though the base file has more.
        assert_eq!(r.read(&mut [0u8; 4]).unwrap(), 0);

        r.seek(SeekFrom::Current(-2)).unwrap();
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"56");
    }

    /// All formats list the same 14 files. Directory counts vary by
    /// format (zip omits the archive root by convention; tar/7z
    /// include it as `./`). Total uncompressed size is consistent.
    #[test]
    fn list_zip_finds_expected_entries() {
        let (entries, _) = list_entries(&fixture("archive.zip"), ArchiveFormat::Zip).unwrap();
        let stats = Stats::from_root(ArchiveFormat::Zip.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_finds_expected_entries() {
        let (entries, _) = list_entries(&fixture("archive.tar"), ArchiveFormat::Tar).unwrap();
        let stats = Stats::from_root(ArchiveFormat::Tar.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_gz_finds_expected_entries() {
        let (entries, _) = list_entries(&fixture("archive.tar.gz"), ArchiveFormat::TarGz).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarGz.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_bz2_finds_expected_entries() {
        let (entries, _) =
            list_entries(&fixture("archive.tar.bz2"), ArchiveFormat::TarBz2).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarBz2.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_xz_finds_expected_entries() {
        let (entries, _) = list_entries(&fixture("archive.tar.xz"), ArchiveFormat::TarXz).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarXz.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_zst_finds_expected_entries() {
        let (entries, _) =
            list_entries(&fixture("archive.tar.zst"), ArchiveFormat::TarZst).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarZst.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_lz4_finds_expected_entries() {
        let (entries, _) =
            list_entries(&fixture("archive.tar.lz4"), ArchiveFormat::TarLz4).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarLz4.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_tar_br_finds_expected_entries() {
        let (entries, _) = list_entries(&fixture("archive.tar.br"), ArchiveFormat::TarBr).unwrap();
        let stats = Stats::from_root(ArchiveFormat::TarBr.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_cpio_finds_expected_entries() {
        let (entries, _) = list_entries(&fixture("archive.cpio"), ArchiveFormat::Cpio).unwrap();
        let stats = Stats::from_root(ArchiveFormat::Cpio.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_cpio_gz_finds_expected_entries() {
        let (entries, _) =
            list_entries(&fixture("archive.cpio.gz"), ArchiveFormat::CpioGz).unwrap();
        let stats = Stats::from_root(ArchiveFormat::CpioGz.label(), &entries);
        assert_eq!(stats.file_count, 14);
        assert_eq!(stats.dir_count, 2);
        assert_eq!(stats.total_size, 30_683);
    }

    #[test]
    fn list_ar_finds_deb_members() {
        // hello.deb is a 3-member ar archive: debian-binary,
        // control.tar.gz, data.tar.gz.
        let (entries, _) = list_entries(&fixture("hello.deb"), ArchiveFormat::Ar).unwrap();
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
        let (entries, _) = list_entries(&fixture("archive.7z"), ArchiveFormat::SevenZ).unwrap();
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
        let (entries, _) = list_entries(&src, ArchiveFormat::Tar).unwrap();
        assert!(entries.is_empty());
    }

    fn zip_with_entry(name: &str, data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        w.start_file(name, opts).unwrap();
        w.write_all(data).unwrap();
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn read_zip_entry_refuses_over_cap_declared_size() {
        let data = vec![0u8; (RENDER_MAX_BYTES + 1) as usize];
        let bytes = zip_with_entry("big.xml", &data);
        let src = InputSource::memory(bytes::Bytes::from(bytes), "big.zip");
        let mut zip = open_zip(&src, "TEST").unwrap();
        let err = read_zip_entry(&mut zip, "big.xml", "TEST").unwrap_err();
        assert!(format!("{err:#}").contains("render cap"), "got: {err:#}");
    }

    /// A zip bomb can lie *small*: declare a 1 KB entry but carry deflate
    /// data that expands to gigabytes. The declared-size gate alone misses
    /// that — the read itself must stop at the cap. Patches the recorded
    /// uncompressed sizes (local header at offset 22, central directory
    /// header at offset 24) down to 1 KB and asserts refusal.
    #[test]
    fn read_zip_entry_bounds_actual_decompression_when_declared_size_lies() {
        let data = vec![0u8; (RENDER_MAX_BYTES + 1) as usize];
        let mut bytes = zip_with_entry("liar.xml", &data);
        let lie = 1024u32.to_le_bytes();
        assert_eq!(&bytes[0..4], b"PK\x03\x04");
        bytes[22..26].copy_from_slice(&lie);
        let cdh = bytes
            .windows(4)
            .rposition(|w| w == b"PK\x01\x02")
            .expect("central directory header");
        bytes[cdh + 24..cdh + 28].copy_from_slice(&lie);

        let src = InputSource::memory(bytes::Bytes::from(bytes), "liar.zip");
        let mut zip = open_zip(&src, "TEST").unwrap();
        let file = zip.by_name("liar.xml").unwrap();
        assert_eq!(file.size(), 1024, "patch must land on the declared size");
        drop(file);
        let err = read_zip_entry(&mut zip, "liar.xml", "TEST").unwrap_err();
        assert!(format!("{err:#}").contains("render cap"), "got: {err:#}");
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
}
