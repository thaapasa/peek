//! Extract a single entry out of an archive (zip / tar / 7z) as an
//! in-memory [`InputSource`].
//!
//! Phase 1: every variant decompresses the entry's full bytes into
//! memory, capped at [`MAX_EXTRACT_BYTES`] to keep hostile archives
//! from triggering OOM. Phase 2 will swap stored zip / uncompressed tar
//! to a `FileRange` view (zero buffering) and spool larger compressed
//! entries to a tempfile.
//!
//! Path safety lives in the shared `extract::sanitize_entry_path`: the
//! key passed by the caller is sanitized before it is matched against
//! any TOC entry, and the entry's own stored path is also sanitized
//! before being adopted as the suggested output name.

use std::io::Read;
use std::path::Path;

use bytes::Bytes;

use crate::extract::{ExtractError, Extracted, sanitize_entry_path};
use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;
use crate::types::archive::reader::open_seekable;

/// Cap on a single extracted entry's size. Phase 1 holds extracts in
/// memory, so a runaway entry would force the process to allocate
/// gigabytes. 256 MB is well past any common-sense need; bigger entries
/// get a clean error rather than an OOM.
const MAX_EXTRACT_BYTES: u64 = 256 * 1024 * 1024;

pub fn extract(
    source: &InputSource,
    format: ArchiveFormat,
    key: &str,
) -> Result<Extracted, ExtractError> {
    let target = sanitize_entry_path(key)?;
    match format {
        ArchiveFormat::Zip => extract_zip(source, &target, key),
        ArchiveFormat::Tar => extract_tar(source, &target, key, TarCompression::None),
        ArchiveFormat::TarGz => extract_tar(source, &target, key, TarCompression::Gz),
        ArchiveFormat::TarBz2 => extract_tar(source, &target, key, TarCompression::Bz2),
        ArchiveFormat::TarXz => extract_tar(source, &target, key, TarCompression::Xz),
        ArchiveFormat::TarZst => extract_tar(source, &target, key, TarCompression::Zst),
        ArchiveFormat::SevenZ => extract_7z(source, &target, key),
    }
}

fn extract_zip(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
) -> Result<Extracted, ExtractError> {
    let reader = open_seekable(source).map_err(ExtractError::Other)?;
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| ExtractError::Other(e.into()))?;

    let target_str = target.to_string_lossy();
    let mut found_idx = None;
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| ExtractError::Other(e.into()))?;
        if file.is_dir() {
            continue;
        }
        let stored = file.name().trim_start_matches('/');
        if stored == target_str {
            found_idx = Some(i);
            break;
        }
    }
    let Some(idx) = found_idx else {
        return Err(ExtractError::NotFound(raw_key.to_string()));
    };
    let mut file = archive
        .by_index(idx)
        .map_err(|e| ExtractError::Other(e.into()))?;
    if file.size() > MAX_EXTRACT_BYTES {
        return Err(ExtractError::Other(anyhow::anyhow!(
            "entry {raw_key:?} is {} bytes; cap is {} bytes",
            file.size(),
            MAX_EXTRACT_BYTES
        )));
    }
    let mut buf = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut buf)
        .map_err(|e| ExtractError::Other(e.into()))?;
    Ok(in_memory_extract(target, buf))
}

#[derive(Clone, Copy)]
enum TarCompression {
    None,
    Gz,
    Bz2,
    Xz,
    Zst,
}

fn extract_tar(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
    compression: TarCompression,
) -> Result<Extracted, ExtractError> {
    let raw = source.read_bytes().map_err(ExtractError::Other)?;
    let decompressed = decompress_tar(&raw, compression)?;
    let mut archive = tar::Archive::new(std::io::Cursor::new(&decompressed[..]));
    let entries = archive
        .entries()
        .map_err(|e| ExtractError::Other(e.into()))?;
    let target_str = target.to_string_lossy();
    for entry in entries {
        let mut entry = entry.map_err(|e| ExtractError::Other(e.into()))?;
        let path = entry
            .path()
            .map_err(|e| ExtractError::Other(e.into()))?
            .into_owned();
        let path_str = path.to_string_lossy();
        let stored = path_str.trim_start_matches("./").trim_start_matches('/');
        if stored != target_str.as_ref() {
            continue;
        }
        let size = entry.size();
        if size > MAX_EXTRACT_BYTES {
            return Err(ExtractError::Other(anyhow::anyhow!(
                "entry {raw_key:?} is {size} bytes; cap is {MAX_EXTRACT_BYTES} bytes"
            )));
        }
        let mut buf = Vec::with_capacity(size as usize);
        entry
            .read_to_end(&mut buf)
            .map_err(|e| ExtractError::Other(e.into()))?;
        return Ok(in_memory_extract(target, buf));
    }
    Err(ExtractError::NotFound(raw_key.to_string()))
}

fn decompress_tar(raw: &[u8], compression: TarCompression) -> Result<Vec<u8>, ExtractError> {
    match compression {
        TarCompression::None => Ok(raw.to_vec()),
        TarCompression::Gz => {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(raw)
                .read_to_end(&mut out)
                .map_err(|e| ExtractError::Other(e.into()))?;
            Ok(out)
        }
        TarCompression::Bz2 => {
            let mut out = Vec::new();
            bzip2::read::BzDecoder::new(raw)
                .read_to_end(&mut out)
                .map_err(|e| ExtractError::Other(e.into()))?;
            Ok(out)
        }
        TarCompression::Xz => {
            let mut out = Vec::new();
            let mut input = std::io::BufReader::new(raw);
            lzma_rs::xz_decompress(&mut input, &mut out)
                .map_err(|e| ExtractError::Other(anyhow::anyhow!("{e:?}")))?;
            Ok(out)
        }
        TarCompression::Zst => {
            let mut out = Vec::new();
            zstd::stream::copy_decode(raw, &mut out).map_err(|e| ExtractError::Other(e.into()))?;
            Ok(out)
        }
    }
}

fn extract_7z(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
) -> Result<Extracted, ExtractError> {
    let reader = open_seekable(source).map_err(ExtractError::Other)?;
    let mut archive = sevenz_rust2::ArchiveReader::new(reader, sevenz_rust2::Password::empty())
        .map_err(|e| ExtractError::Other(anyhow::anyhow!("{e}")))?;

    let target_str = target.to_string_lossy();
    // Validate the entry exists and respect size cap before decompressing.
    let entry = archive
        .archive()
        .files
        .iter()
        .find(|e| !e.is_directory() && e.name().trim_start_matches('/') == target_str)
        .ok_or_else(|| ExtractError::NotFound(raw_key.to_string()))?;
    if entry.size() > MAX_EXTRACT_BYTES {
        return Err(ExtractError::Other(anyhow::anyhow!(
            "entry {raw_key:?} is {} bytes; cap is {MAX_EXTRACT_BYTES} bytes",
            entry.size()
        )));
    }
    let buf = archive
        .read_file(&target_str)
        .map_err(|e| ExtractError::Other(anyhow::anyhow!("{e}")))?;
    Ok(in_memory_extract(target, buf))
}

fn in_memory_extract(target: &Path, buf: Vec<u8>) -> Extracted {
    let suggested_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extracted")
        .to_string();
    Extracted {
        source: InputSource::memory(Bytes::from(buf), suggested_name.clone()),
        suggested_name,
    }
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

    /// All shared archive fixtures contain `fibonacci.py` at the root —
    /// 14 files total per the listing tests. Using one entry across
    /// every backend keeps the extract tests structurally identical.
    const STABLE_ENTRY: &str = "fibonacci.py";

    #[test]
    fn extract_zip_returns_known_entry() {
        let extracted = extract(&fixture("archive.zip"), ArchiveFormat::Zip, STABLE_ENTRY)
            .expect("zip extract");
        assert_eq!(extracted.suggested_name, "fibonacci.py");
        let bytes = extracted.source.read_bytes().unwrap();
        assert_eq!(bytes.len(), 2_250, "fibonacci.py is 2250 bytes");
    }

    #[test]
    fn extract_tar_gz_returns_known_entry() {
        let extracted = extract(
            &fixture("archive.tar.gz"),
            ArchiveFormat::TarGz,
            STABLE_ENTRY,
        )
        .expect("tar.gz extract");
        assert_eq!(extracted.suggested_name, "fibonacci.py");
        let bytes = extracted.source.read_bytes().unwrap();
        assert_eq!(bytes.len(), 2_250);
    }

    #[test]
    fn extract_seven_z_returns_known_entry() {
        let extracted = extract(&fixture("archive.7z"), ArchiveFormat::SevenZ, STABLE_ENTRY)
            .expect("7z extract");
        assert_eq!(extracted.suggested_name, "fibonacci.py");
        let bytes = extracted.source.read_bytes().unwrap();
        assert_eq!(bytes.len(), 2_250);
    }

    #[test]
    fn missing_entry_errors() {
        let err = extract(
            &fixture("archive.zip"),
            ArchiveFormat::Zip,
            "no/such/file.txt",
        )
        .unwrap_err();
        assert!(matches!(err, ExtractError::NotFound(_)));
    }

    #[test]
    fn traversal_rejected_before_lookup() {
        let err =
            extract(&fixture("archive.zip"), ArchiveFormat::Zip, "../etc/passwd").unwrap_err();
        assert!(matches!(err, ExtractError::UnsafePath(_)));
    }
}
