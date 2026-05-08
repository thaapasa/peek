//! Extract a single entry out of an archive (zip / tar / 7z) as an
//! in-memory [`InputSource`]. The entry is decompressed into a
//! [`Bytes`] buffer capped at [`MAX_EXTRACT_BYTES`] — bigger entries
//! error out rather than triggering OOM.
//!
//! Path safety: keys go through `extract::sanitize_entry_path` before
//! any TOC lookup so traversal (`..`) is rejected.

use std::io::Read;
use std::path::Path;

use bytes::Bytes;

use crate::extract::{ExtractError, Extracted, sanitize_entry_path};
use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;
use crate::types::archive::reader::open_seekable;

/// Hard cap on a single extracted entry. Extracts land in memory, so a
/// runaway entry would force a multi-GB allocation; anything past this
/// errors out cleanly.
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
        ArchiveFormat::Ar => extract_ar(source, &target, key),
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

/// Extract a single ar entry. ar uses 60-byte ASCII headers; walk
/// the chain, match the requested name, copy the payload bytes.
fn extract_ar(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
) -> Result<Extracted, ExtractError> {
    use std::io::Read;
    const HEADER_LEN: usize = 60;
    const GLOBAL_MAGIC: &[u8; 8] = b"!<arch>\n";

    let mut reader =
        crate::types::archive::reader::open_seekable(source).map_err(ExtractError::Other)?;
    let mut magic = [0u8; 8];
    reader
        .read_exact(&mut magic)
        .map_err(|e| ExtractError::Other(e.into()))?;
    if &magic != GLOBAL_MAGIC {
        return Err(ExtractError::Other(anyhow::anyhow!(
            "not an ar archive: missing !<arch> magic"
        )));
    }

    let target_str = target.to_string_lossy();
    let mut header = [0u8; HEADER_LEN];
    loop {
        let n = reader
            .read(&mut header)
            .map_err(|e| ExtractError::Other(e.into()))?;
        if n == 0 || n < HEADER_LEN {
            break;
        }
        let raw_name = std::str::from_utf8(&header[..16])
            .unwrap_or("")
            .trim_end_matches(' ')
            .trim_end_matches('/')
            .to_string();
        let total_size: u64 = std::str::from_utf8(&header[48..58])
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        // BSD long name: `#1/<len>` header, name in payload prefix.
        let (name, payload_size) = if let Some(rest) = raw_name.strip_prefix("#1/") {
            let name_len: u64 = rest.trim().parse().unwrap_or(0);
            if name_len > total_size {
                ("?".to_string(), total_size)
            } else {
                let mut nbuf = vec![0u8; name_len as usize];
                reader
                    .read_exact(&mut nbuf)
                    .map_err(|e| ExtractError::Other(e.into()))?;
                let n = std::str::from_utf8(&nbuf)
                    .unwrap_or("?")
                    .trim_end_matches('\0')
                    .to_string();
                (n, total_size - name_len)
            }
        } else {
            (raw_name, total_size)
        };
        let pad = total_size % 2;

        if name == target_str {
            if payload_size > MAX_EXTRACT_BYTES {
                return Err(ExtractError::Other(anyhow::anyhow!(
                    "entry {raw_key:?} is {payload_size} bytes; cap is {MAX_EXTRACT_BYTES} bytes"
                )));
            }
            let mut buf = vec![0u8; payload_size as usize];
            reader
                .read_exact(&mut buf)
                .map_err(|e| ExtractError::Other(e.into()))?;
            return Ok(in_memory_extract(target, buf));
        }

        let mut skip = vec![0u8; (payload_size + pad) as usize];
        reader
            .read_exact(&mut skip)
            .map_err(|e| ExtractError::Other(e.into()))?;
    }
    Err(ExtractError::NotFound(raw_key.to_string()))
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
