//! Extract a single entry out of an archive (zip / tar / 7z / ar / cpio)
//! as a fresh [`InputSource`]. Large entries spool to an
//! [`InputSource::TempFile`] in `$TMPDIR/peek-*` so multi-GB payloads
//! don't have to fit in RAM; smaller entries stay in
//! [`InputSource::Memory`]. `--no-tempfile` (carried on
//! [`ExtractOptions::no_tempfile`]) forces the in-memory path
//! unconditionally and drops the [`MAX_EXTRACT_BYTES`] safety cap.
//!
//! Path safety: keys go through `extract::sanitize_entry_path` before
//! any TOC lookup so traversal (`..`) is rejected.

use std::io::{Cursor, Read};
use std::path::Path;

use bytes::Bytes;
use tempfile::Builder as TempBuilder;

use crate::extract::{
    ExtractError, ExtractOptions, Extracted, forward_slash_key, sanitize_entry_path,
};
use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;
use crate::types::archive::reader::open_seekable;

/// Hard cap on a single in-memory extracted entry. Only enforced on
/// the `Vec<u8>` fallback path — the spool-to-tempfile path bypasses
/// this since disk, not RAM, is the limit. Setting `--no-tempfile`
/// drops the cap as well: the user explicitly chose the memory path.
const MAX_EXTRACT_BYTES: u64 = 256 * 1024 * 1024;

/// Spool threshold: at or above this many bytes (or when the entry's
/// declared size is unknown), [`materialise`] writes to a
/// [`tempfile::NamedTempFile`] instead of an in-memory `Vec<u8>`.
/// Small entries stay in `Vec<u8>` so the common case avoids tempdir
/// syscalls.
const SPOOL_THRESHOLD: u64 = 16 * 1024 * 1024;

pub fn extract(
    source: &InputSource,
    format: ArchiveFormat,
    key: &str,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    let target = sanitize_entry_path(key)?;
    match format {
        ArchiveFormat::Zip => extract_zip(source, &target, key, opts),
        ArchiveFormat::Tar => extract_tar(source, &target, key, TarCompression::None, opts),
        ArchiveFormat::TarGz => extract_tar(source, &target, key, TarCompression::Gz, opts),
        ArchiveFormat::TarBz2 => extract_tar(source, &target, key, TarCompression::Bz2, opts),
        ArchiveFormat::TarXz => extract_tar(source, &target, key, TarCompression::Xz, opts),
        ArchiveFormat::TarZst => extract_tar(source, &target, key, TarCompression::Zst, opts),
        ArchiveFormat::TarLz4 => extract_tar(source, &target, key, TarCompression::Lz4, opts),
        ArchiveFormat::SevenZ => extract_7z(source, &target, key, opts),
        ArchiveFormat::Ar => extract_ar(source, &target, key, opts),
        ArchiveFormat::Cpio => extract_cpio(source, &target, key, CpioCompression::None, opts),
        ArchiveFormat::CpioGz => extract_cpio(source, &target, key, CpioCompression::Gz, opts),
    }
}

/// Stream `reader` (with optional `declared_size` hint) into a fresh
/// [`InputSource`]. Picks between a [`tempfile::NamedTempFile`] spool
/// and an in-memory `Vec<u8>` based on size + `opts.no_tempfile`. On
/// tempfile creation failure, falls back to in-memory (reader has not
/// been touched yet, so the fallback is safe). On in-flight spool
/// write failure, no fallback is possible and the error surfaces.
fn materialise<R: Read>(
    mut reader: R,
    declared_size: Option<u64>,
    target: &Path,
    raw_key: &str,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    let suggested_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extracted")
        .to_string();

    // User-forced memory path: skip spool, no cap.
    if opts.no_tempfile {
        return read_to_memory(&mut reader, declared_size, suggested_name, raw_key, true);
    }

    let want_spool = declared_size.is_none_or(|s| s >= SPOOL_THRESHOLD);
    if want_spool {
        match TempBuilder::new().prefix("peek-").tempfile() {
            Ok(mut tmp) => {
                std::io::copy(&mut reader, tmp.as_file_mut()).map_err(|e| {
                    ExtractError::Other(
                        anyhow::Error::from(e)
                            .context(format!("tempfile spool of {raw_key:?} failed")),
                    )
                })?;
                return Ok(Extracted {
                    source: InputSource::temp_file(tmp, suggested_name.clone()),
                    suggested_name,
                });
            }
            Err(e) => {
                // Tempfile creation failed. Reader hasn't been
                // consumed — fall through to the in-memory path.
                eprintln!("peek: tempfile create failed ({e}); falling back to in-memory extract");
            }
        }
    }

    read_to_memory(&mut reader, declared_size, suggested_name, raw_key, false)
}

fn read_to_memory<R: Read>(
    reader: &mut R,
    declared_size: Option<u64>,
    suggested_name: String,
    raw_key: &str,
    skip_cap: bool,
) -> Result<Extracted, ExtractError> {
    if !skip_cap
        && let Some(sz) = declared_size
        && sz > MAX_EXTRACT_BYTES
    {
        return Err(ExtractError::Other(anyhow::anyhow!(
            "entry {raw_key:?} is {sz} bytes; in-memory cap is {MAX_EXTRACT_BYTES} bytes (re-run with --no-tempfile to override, or check tempfile errors above)"
        )));
    }
    let mut buf = Vec::with_capacity(declared_size.unwrap_or(0) as usize);
    reader
        .read_to_end(&mut buf)
        .map_err(|e| ExtractError::Other(e.into()))?;
    if !skip_cap && buf.len() as u64 > MAX_EXTRACT_BYTES {
        return Err(ExtractError::Other(anyhow::anyhow!(
            "entry {raw_key:?} produced {} bytes; in-memory cap is {MAX_EXTRACT_BYTES} bytes",
            buf.len()
        )));
    }
    Ok(Extracted {
        source: InputSource::memory(Bytes::from(buf), suggested_name.clone()),
        suggested_name,
    })
}

#[derive(Clone, Copy)]
enum CpioCompression {
    None,
    Gz,
}

fn extract_cpio(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
    compression: CpioCompression,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    // cpio's hand-rolled reader buffers the matching body into a
    // `Bytes` internally; we then route it through `materialise` so
    // large entries still spool to disk (the cpio Vec is dropped after
    // the copy, leaving only the tempfile).
    let raw = source.read_bytes().map_err(ExtractError::Other)?;
    let target_str = forward_slash_key(target);
    let found = match compression {
        CpioCompression::None => crate::types::archive::backends::cpio::find_entry(
            std::io::Cursor::new(&raw[..]),
            &target_str,
            u64::MAX,
        ),
        CpioCompression::Gz => crate::types::archive::backends::cpio::find_entry(
            flate2::read::GzDecoder::new(std::io::Cursor::new(&raw[..])),
            &target_str,
            u64::MAX,
        ),
    }
    .map_err(ExtractError::Other)?;
    let Some(body) = found else {
        return Err(ExtractError::NotFound(raw_key.to_string()));
    };
    let size = body.len() as u64;
    materialise(Cursor::new(body), Some(size), target, raw_key, opts)
}

fn extract_zip(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    let reader = open_seekable(source).map_err(ExtractError::Other)?;
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| ExtractError::Other(e.into()))?;

    let target_str = forward_slash_key(target);
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
    let file = archive
        .by_index(idx)
        .map_err(|e| ExtractError::Other(e.into()))?;
    let size = file.size();
    materialise(file, Some(size), target, raw_key, opts)
}

#[derive(Clone, Copy)]
enum TarCompression {
    None,
    Gz,
    Bz2,
    Xz,
    Zst,
    Lz4,
}

fn extract_tar(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
    compression: TarCompression,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    let raw = source.read_bytes().map_err(ExtractError::Other)?;
    let decompressed = decompress_tar(&raw, compression)?;
    let mut archive = tar::Archive::new(std::io::Cursor::new(decompressed.as_ref()));
    let entries = archive
        .entries()
        .map_err(|e| ExtractError::Other(e.into()))?;
    let target_str = forward_slash_key(target);
    for entry in entries {
        let entry = entry.map_err(|e| ExtractError::Other(e.into()))?;
        let path = entry
            .path()
            .map_err(|e| ExtractError::Other(e.into()))?
            .into_owned();
        let path_str = forward_slash_key(&path);
        let stored = path_str.trim_start_matches("./").trim_start_matches('/');
        if stored != target_str.as_str() {
            continue;
        }
        let size = entry.size();
        return materialise(entry, Some(size), target, raw_key, opts);
    }
    Err(ExtractError::NotFound(raw_key.to_string()))
}

/// Decompress a compressed tar payload. Delegates codec dispatch to
/// [`crate::input::compression::decompress_bytes`] so the same five
/// codec implementations cover both transparent single-stream
/// decompression and tar extraction. None arm refcount-clones the
/// input `Bytes` (no copy).
fn decompress_tar(raw: &Bytes, compression: TarCompression) -> Result<Bytes, ExtractError> {
    use crate::input::compression::decompress_bytes;
    use crate::input::detect::CompressionFormat;
    let fmt = match compression {
        TarCompression::None => return Ok(raw.clone()),
        TarCompression::Gz => CompressionFormat::Gz,
        TarCompression::Bz2 => CompressionFormat::Bz2,
        TarCompression::Xz => CompressionFormat::Xz,
        TarCompression::Zst => CompressionFormat::Zst,
        TarCompression::Lz4 => CompressionFormat::Lz4,
    };
    decompress_bytes(raw.as_ref(), fmt).map_err(ExtractError::Other)
}

fn extract_7z(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    let reader = open_seekable(source).map_err(ExtractError::Other)?;
    let mut archive = sevenz_rust2::ArchiveReader::new(reader, sevenz_rust2::Password::empty())
        .map_err(|e| ExtractError::Other(anyhow::anyhow!("{e}")))?;

    let target_str = forward_slash_key(target);
    let size = archive
        .archive()
        .files
        .iter()
        .find(|e| !e.is_directory() && e.name().trim_start_matches('/') == target_str)
        .ok_or_else(|| ExtractError::NotFound(raw_key.to_string()))?
        .size();
    // sevenz-rust2 exposes only a Vec-returning `read_file`; pipe the
    // resulting Vec through `materialise` so large entries still
    // spool to disk and the Vec drops after copy.
    let buf = archive
        .read_file(&target_str)
        .map_err(|e| ExtractError::Other(anyhow::anyhow!("{e}")))?;
    materialise(Cursor::new(buf), Some(size), target, raw_key, opts)
}

/// Extract a single ar entry. ar uses 60-byte ASCII headers; walk
/// the chain, match the requested name, copy the payload bytes.
fn extract_ar(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
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

    let target_str = forward_slash_key(target);
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
            let body = (&mut reader).take(payload_size);
            return materialise(body, Some(payload_size), target, raw_key, opts);
        }

        let mut skip = vec![0u8; (payload_size + pad) as usize];
        reader
            .read_exact(&mut skip)
            .map_err(|e| ExtractError::Other(e.into()))?;
    }
    Err(ExtractError::NotFound(raw_key.to_string()))
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

    fn opts() -> ExtractOptions {
        ExtractOptions::default()
    }

    /// All shared archive fixtures contain `fibonacci.py` at the root —
    /// 14 files total per the listing tests. Using one entry across
    /// every backend keeps the extract tests structurally identical.
    const STABLE_ENTRY: &str = "fibonacci.py";

    /// All shared archive fixtures also contain `config/theme.rs` —
    /// a nested entry used to exercise subdirectory lookups. The bug
    /// this guards against: building the lookup key with
    /// `PathBuf::to_string_lossy()` after a component-by-component
    /// `push` uses the host OS separator (`\` on Windows), so
    /// comparing against archive members (always `/`-separated) fails.
    const SUBPATH_ENTRY: &str = "config/theme.rs";
    const SUBPATH_ENTRY_SIZE: usize = 2_956;

    #[test]
    fn extract_zip_returns_known_entry() {
        let extracted = extract(
            &fixture("archive.zip"),
            ArchiveFormat::Zip,
            STABLE_ENTRY,
            &opts(),
        )
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
            &opts(),
        )
        .expect("tar.gz extract");
        assert_eq!(extracted.suggested_name, "fibonacci.py");
        let bytes = extracted.source.read_bytes().unwrap();
        assert_eq!(bytes.len(), 2_250);
    }

    #[test]
    fn extract_tar_lz4_returns_known_entry() {
        let extracted = extract(
            &fixture("archive.tar.lz4"),
            ArchiveFormat::TarLz4,
            STABLE_ENTRY,
            &opts(),
        )
        .expect("tar.lz4 extract");
        assert_eq!(extracted.suggested_name, "fibonacci.py");
        let bytes = extracted.source.read_bytes().unwrap();
        assert_eq!(bytes.len(), 2_250);
    }

    #[test]
    fn extract_cpio_returns_known_entry() {
        let extracted = extract(
            &fixture("archive.cpio"),
            ArchiveFormat::Cpio,
            STABLE_ENTRY,
            &opts(),
        )
        .expect("cpio extract");
        assert_eq!(extracted.suggested_name, "fibonacci.py");
        let bytes = extracted.source.read_bytes().unwrap();
        assert_eq!(bytes.len(), 2_250);
    }

    #[test]
    fn extract_cpio_gz_returns_known_entry() {
        let extracted = extract(
            &fixture("archive.cpio.gz"),
            ArchiveFormat::CpioGz,
            STABLE_ENTRY,
            &opts(),
        )
        .expect("cpio.gz extract");
        assert_eq!(extracted.suggested_name, "fibonacci.py");
        let bytes = extracted.source.read_bytes().unwrap();
        assert_eq!(bytes.len(), 2_250);
    }

    #[test]
    fn extract_seven_z_returns_known_entry() {
        let extracted = extract(
            &fixture("archive.7z"),
            ArchiveFormat::SevenZ,
            STABLE_ENTRY,
            &opts(),
        )
        .expect("7z extract");
        assert_eq!(extracted.suggested_name, "fibonacci.py");
        let bytes = extracted.source.read_bytes().unwrap();
        assert_eq!(bytes.len(), 2_250);
    }

    /// Extract a forward-slash subpath through every archive backend
    /// that exposes nested entries. Guards against the Windows-only
    /// regression where the sanitized lookup key carried backslashes
    /// and never matched the archive's stored entry names.
    #[test]
    fn extract_subpath_entry_across_backends() {
        let cases: &[(&str, ArchiveFormat)] = &[
            ("archive.zip", ArchiveFormat::Zip),
            ("archive.tar", ArchiveFormat::Tar),
            ("archive.tar.gz", ArchiveFormat::TarGz),
            ("archive.tar.bz2", ArchiveFormat::TarBz2),
            ("archive.tar.xz", ArchiveFormat::TarXz),
            ("archive.tar.zst", ArchiveFormat::TarZst),
            ("archive.tar.lz4", ArchiveFormat::TarLz4),
            ("archive.7z", ArchiveFormat::SevenZ),
            ("archive.cpio", ArchiveFormat::Cpio),
            ("archive.cpio.gz", ArchiveFormat::CpioGz),
        ];
        for (name, format) in cases {
            let extracted = extract(&fixture(name), *format, SUBPATH_ENTRY, &opts())
                .unwrap_or_else(|e| panic!("{name} subpath extract: {e}"));
            assert_eq!(extracted.suggested_name, "theme.rs", "{name}");
            let bytes = extracted.source.read_bytes().unwrap();
            assert_eq!(bytes.len(), SUBPATH_ENTRY_SIZE, "{name}");
        }
    }

    #[test]
    fn missing_entry_errors() {
        let err = extract(
            &fixture("archive.zip"),
            ArchiveFormat::Zip,
            "no/such/file.txt",
            &opts(),
        )
        .unwrap_err();
        assert!(matches!(err, ExtractError::NotFound(_)));
    }

    #[test]
    fn traversal_rejected_before_lookup() {
        let err = extract(
            &fixture("archive.zip"),
            ArchiveFormat::Zip,
            "../etc/passwd",
            &opts(),
        )
        .unwrap_err();
        assert!(matches!(err, ExtractError::UnsafePath(_)));
    }

    /// Empty `.tar` extract: walking the (empty) entry list must
    /// finish and return `NotFound` rather than hanging in the tar
    /// reader. Pairs with the listing-side empty-tar test.
    #[test]
    fn extract_from_empty_tar_returns_not_found() {
        let src = InputSource::memory(bytes::Bytes::new(), "empty.tar");
        let err = extract(&src, ArchiveFormat::Tar, "anything", &opts()).unwrap_err();
        assert!(matches!(err, ExtractError::NotFound(_)));
    }

    /// Spool path: extract an entry whose declared size meets the
    /// `SPOOL_THRESHOLD`. With default `opts.no_tempfile = false`,
    /// the entry should land as `InputSource::TempFile`, the
    /// suggested name should match the entry's basename, and reads
    /// over the resulting source should return the entry contents.
    #[test]
    fn materialise_spools_large_payload_to_tempfile() {
        // 16 MiB of zeroes; SPOOL_THRESHOLD is exactly 16 MiB.
        let payload: Vec<u8> = vec![0u8; 16 * 1024 * 1024];
        let target = Path::new("big.bin");
        let res = materialise(
            Cursor::new(payload.clone()),
            Some(payload.len() as u64),
            target,
            "big.bin",
            &ExtractOptions::default(),
        )
        .expect("spool succeeds");
        assert!(
            matches!(res.source, InputSource::TempFile { .. }),
            "expected TempFile, got {:?}",
            res.source
        );
        let bytes = res.source.read_bytes().unwrap();
        assert_eq!(bytes.len(), payload.len());
    }

    /// Recursive spool regression: outer.zip contains nested.zip
    /// (≥ SPOOL_THRESHOLD), which contains a small `leaf.txt`.
    /// First extract spools the inner zip to a `TempFile`. Second
    /// extract opens that `TempFile`-backed source as a zip archive
    /// and pulls `leaf.txt` out — exercising the `open_seekable`
    /// `TempFile` arm + the `Arc<NamedTempFile>` lifetime carried by
    /// `Extracted::source` through a second pass of `extract::extract`.
    #[test]
    fn extract_recurses_through_tempfile_source() {
        use bytes::Bytes;
        use zip::CompressionMethod;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
            let cursor = std::io::Cursor::new(Vec::<u8>::new());
            let mut w = ZipWriter::new(cursor);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            for (name, data) in entries {
                w.start_file(*name, opts).unwrap();
                std::io::Write::write_all(&mut w, data).unwrap();
            }
            w.finish().unwrap().into_inner()
        }

        let leaf = b"hello peek recursive".to_vec();
        // Pad inner.zip past SPOOL_THRESHOLD so extracting it from the
        // outer zip lands on the tempfile path.
        let pad = vec![0u8; SPOOL_THRESHOLD as usize];
        let inner_zip = build_zip(&[("leaf.txt", &leaf), ("pad.bin", &pad)]);
        assert!(
            inner_zip.len() as u64 >= SPOOL_THRESHOLD,
            "inner.zip must cross spool threshold"
        );
        let outer_zip = build_zip(&[("nested.zip", &inner_zip)]);

        let outer_src = InputSource::memory(Bytes::from(outer_zip), "outer.zip");
        let first =
            extract(&outer_src, ArchiveFormat::Zip, "nested.zip", &opts()).expect("outer extract");
        assert!(
            matches!(first.source, InputSource::TempFile { .. }),
            "inner.zip should spool, got {:?}",
            first.source
        );

        // Recurse: extract `leaf.txt` from the spooled inner zip. This
        // exercises `open_seekable` on the `TempFile` variant.
        let second = extract(&first.source, ArchiveFormat::Zip, "leaf.txt", &opts())
            .expect("nested extract through TempFile source");
        assert_eq!(second.suggested_name, "leaf.txt");
        let bytes = second.source.read_bytes().unwrap();
        assert_eq!(bytes.as_ref(), leaf.as_slice());
    }

    /// `--no-tempfile` keeps the buffer in `Vec<u8>` even when it
    /// crosses the spool threshold, and bypasses the safety cap so
    /// arbitrarily large entries are allowed.
    #[test]
    fn materialise_respects_no_tempfile_override() {
        let payload: Vec<u8> = vec![0u8; 16 * 1024 * 1024];
        let res = materialise(
            Cursor::new(payload.clone()),
            Some(payload.len() as u64),
            Path::new("big.bin"),
            "big.bin",
            &ExtractOptions {
                no_tempfile: true,
                ..Default::default()
            },
        )
        .expect("memory path succeeds under --no-tempfile");
        assert!(
            matches!(res.source, InputSource::Memory { .. }),
            "expected Memory, got {:?}",
            res.source
        );
        assert_eq!(res.source.read_bytes().unwrap().len(), payload.len());
    }
}
