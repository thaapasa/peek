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

use std::io::Read;
use std::path::Path;

use bytes::Bytes;
use tempfile::Builder as TempBuilder;

use crate::extract::{
    ExtractError, ExtractOptions, Extracted, forward_slash_key, sanitize_entry_path,
};
use crate::input::InputSource;
use crate::input::detect::{ArchiveFormat, CompressionFormat};
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
        ArchiveFormat::TarBr => extract_tar(source, &target, key, TarCompression::Br, opts),
        ArchiveFormat::SevenZ => extract_7z(source, &target, key, opts),
        ArchiveFormat::Ar => extract_ar(source, &target, key, opts),
        ArchiveFormat::Cpio => extract_cpio(source, &target, key, CpioCompression::None, opts),
        ArchiveFormat::CpioGz => extract_cpio(source, &target, key, CpioCompression::Gz, opts),
    }
}

/// Basename of an entry path, used as the extracted source's display
/// name. Falls back to `extracted` for pathological keys.
fn suggested_name(target: &Path) -> String {
    target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extracted")
        .to_string()
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
    let suggested_name = suggested_name(target);

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
    // Stream the cpio walk over the source — no whole-archive read into
    // RAM. `find_entry` stops at the match and hands back a size-limited
    // body reader (never the whole body buffered), which `materialise`
    // then spools or caps. The matched entry is streamed straight through.
    use crate::types::archive::backends::cpio::{CpioBody, find_entry};
    let reader = open_seekable(source).map_err(ExtractError::Other)?;
    let target_str = forward_slash_key(target);

    fn finish<R: Read>(
        found: Option<(CpioBody<R>, u64)>,
        target: &Path,
        raw_key: &str,
        opts: &ExtractOptions,
    ) -> Result<Extracted, ExtractError> {
        let Some((body, size)) = found else {
            return Err(ExtractError::NotFound(raw_key.to_string()));
        };
        materialise(body, Some(size), target, raw_key, opts)
    }

    match compression {
        CpioCompression::None => finish(
            find_entry(reader, &target_str).map_err(ExtractError::Other)?,
            target,
            raw_key,
            opts,
        ),
        CpioCompression::Gz => finish(
            find_entry(flate2::read::GzDecoder::new(reader), &target_str)
                .map_err(ExtractError::Other)?,
            target,
            raw_key,
            opts,
        ),
    }
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
    // Stored (uncompressed) + unencrypted entries are a verbatim slice of
    // the backing source: the stored bytes are the content. Hand back a
    // zero-copy range instead of spooling. Anything compressed or
    // encrypted falls through to `materialise`.
    if file.compression() == zip::CompressionMethod::Stored
        && !file.encrypted()
        && let Some(data_start) = file.data_start()
    {
        let suggested_name = suggested_name(target);
        let src = source.subrange(data_start, size, &suggested_name);
        return Ok(Extracted {
            source: src,
            suggested_name,
        });
    }
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
    Br,
}

impl TarCompression {
    /// Codec key for a compressed tar; `None` for plain (uncompressed).
    fn format(self) -> Option<CompressionFormat> {
        match self {
            TarCompression::None => None,
            TarCompression::Gz => Some(CompressionFormat::Gz),
            TarCompression::Bz2 => Some(CompressionFormat::Bz2),
            TarCompression::Xz => Some(CompressionFormat::Xz),
            TarCompression::Zst => Some(CompressionFormat::Zst),
            TarCompression::Lz4 => Some(CompressionFormat::Lz4),
            TarCompression::Br => Some(CompressionFormat::Br),
        }
    }
}

fn extract_tar(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
    compression: TarCompression,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    let reader = open_seekable(source).map_err(ExtractError::Other)?;
    let target_str = forward_slash_key(target);
    match compression.format() {
        // Plain tar: walk the seekable reader directly so non-matching
        // entry bodies are skipped via seek, not read.
        None => walk_tar(reader, source, &target_str, target, true, raw_key, opts),
        // Compressed: stream through the decoder; every codec (xz included)
        // streams, so only the bytes up to the matched entry get inflated.
        Some(fmt) => {
            let dec = crate::types::archive::backends::tar::decode_compressed(reader, fmt)
                .map_err(ExtractError::Other)?;
            walk_tar(dec, source, &target_str, target, false, raw_key, opts)
        }
    }
}

/// Walk a tar stream for `target_str`, returning the matched entry as a
/// fresh source. `uncompressed` selects the extraction strategy: a plain
/// tar member is a verbatim slice of the backing source, so it returns a
/// zero-copy [`InputSource::subrange`] view; a member from a compressed
/// stream is spooled via [`materialise`] as it is read.
fn walk_tar<R: Read>(
    reader: R,
    source: &InputSource,
    target_str: &str,
    target: &Path,
    uncompressed: bool,
    raw_key: &str,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|e| ExtractError::Other(e.into()))?;
    for entry in entries {
        let entry = entry.map_err(|e| ExtractError::Other(e.into()))?;
        let path = entry
            .path()
            .map_err(|e| ExtractError::Other(e.into()))?
            .into_owned();
        let path_str = forward_slash_key(&path);
        let stored = path_str.trim_start_matches("./").trim_start_matches('/');
        if stored != target_str {
            continue;
        }
        let size = entry.size();
        if uncompressed {
            // `raw_file_position` is relative to the tar stream start,
            // which equals the source start when no codec intervenes.
            let suggested_name = suggested_name(target);
            let src = source.subrange(entry.raw_file_position(), size, &suggested_name);
            return Ok(Extracted {
                source: src,
                suggested_name,
            });
        }
        return materialise(entry, Some(size), target, raw_key, opts);
    }
    Err(ExtractError::NotFound(raw_key.to_string()))
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
    // Stream the matched entry through `for_each_entries` (which hands a
    // `&mut dyn Read` per entry) rather than `read_file`, which buffers
    // the whole member into a `Vec` first.
    //
    // 7z packs files into solid blocks sharing one sequential decode
    // stream: an entry's bytes must be consumed before the next entry
    // reads, or that next read is misaligned and its CRC fails. So
    // non-matching entries before the match are drained to advance the
    // stream (the inherent solid-block decode cost; output is discarded,
    // not buffered). Once found, later entries/blocks are skipped without
    // reading so nothing past the match is decoded.
    let mut result: Option<Result<Extracted, ExtractError>> = None;
    let mut found = false;
    archive
        .for_each_entries(|entry, reader| {
            // `ArchiveReader::for_each_entries` ignores the per-block stop
            // and keeps iterating later blocks, so this guard is reached
            // for entries in blocks after the match — skip them cheaply
            // (no drain, no decode) instead of returning `Ok(false)`,
            // which would only stop the current block.
            if found {
                return Ok(true);
            }
            if !entry.is_directory() && entry.name().trim_start_matches('/') == target_str {
                found = true;
                result = Some(materialise(
                    reader,
                    Some(entry.size()),
                    target,
                    raw_key,
                    opts,
                ));
                return Ok(false);
            }
            if let Err(e) = std::io::copy(reader, &mut std::io::sink()) {
                found = true;
                result = Some(Err(ExtractError::Other(e.into())));
                return Ok(false);
            }
            Ok(true)
        })
        .map_err(|e| ExtractError::Other(anyhow::anyhow!("{e}")))?;
    result.unwrap_or_else(|| Err(ExtractError::NotFound(raw_key.to_string())))
}

/// Extract a single ar entry via the shared [`ArReader`] header walk —
/// the same parser the listing path uses. Non-matching members are
/// skipped by `next_entry`'s drain; the matched body streams to
/// `materialise`.
fn extract_ar(
    source: &InputSource,
    target: &Path,
    raw_key: &str,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    use crate::types::archive::backends::ar::ArReader;
    let reader = open_seekable(source).map_err(ExtractError::Other)?;
    let mut ar = ArReader::new(reader).map_err(ExtractError::Other)?;
    let target_str = forward_slash_key(target);
    while let Some(entry) = ar.next_entry().map_err(ExtractError::Other)? {
        if entry.name == target_str {
            let body = ar.body(entry.size);
            return materialise(body, Some(entry.size), target, raw_key, opts);
        }
    }
    Err(ExtractError::NotFound(raw_key.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
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
    fn extract_tar_br_returns_known_entry() {
        let extracted = extract(
            &fixture("archive.tar.br"),
            ArchiveFormat::TarBr,
            STABLE_ENTRY,
            &opts(),
        )
        .expect("tar.br extract");
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

    /// Multi-block 7z: `push_archive_entry` is non-solid, so each entry
    /// lands in its own block. Extracting an entry past the first block
    /// exercises the cross-block path — the outer walk decodes/drains
    /// earlier blocks to reach it, and the `found` guard skips blocks
    /// after the match (which `for_each_entries` still iterates). A wrong
    /// block index or a missing drain would return the wrong member.
    #[test]
    fn extract_7z_multi_block_target_in_later_block() {
        use sevenz_rust2::{ArchiveEntry, ArchiveWriter};

        let payloads: [(&str, &[u8]); 3] = [
            ("a.txt", b"first block contents AAAA"),
            ("b.txt", b"second block contents BBBB"),
            ("c.txt", b"third block contents CCCC"),
        ];
        let mut w = ArchiveWriter::new(Cursor::new(Vec::<u8>::new())).unwrap();
        for (name, data) in payloads {
            w.push_archive_entry(ArchiveEntry::new_file(name), Some(data))
                .unwrap();
        }
        let archive = w.finish().unwrap().into_inner();
        let src = InputSource::memory(bytes::Bytes::from(archive), "multi.7z");

        // Last, middle, then first — covers a target in a trailing block,
        // an interior block, and block 0 (later blocks then skipped).
        for (name, want) in [
            ("c.txt", &payloads[2].1),
            ("b.txt", &payloads[1].1),
            ("a.txt", &payloads[0].1),
        ] {
            let got = extract(&src, ArchiveFormat::SevenZ, name, &opts())
                .unwrap_or_else(|e| panic!("{name}: {e}"))
                .source
                .read_bytes()
                .unwrap();
            assert_eq!(
                got.as_ref(),
                *want,
                "{name} returned the wrong block's bytes"
            );
        }
    }

    /// ar extraction over a `FileRange` source (recursive ar-in-archive):
    /// the header walk runs over `RangeReadSeek`, which can short-read, so
    /// it must use `read_exact`. Extract a trailing member (preceded by
    /// others that get skipped) and cross-check against the File-source
    /// extract.
    #[test]
    fn extract_ar_over_file_range_source() {
        let InputSource::File(path) = fixture("hello.deb") else {
            unreachable!("fixture is a File source");
        };
        let len = std::fs::metadata(&path).unwrap().len();

        let want = extract(
            &InputSource::File(path.clone()),
            ArchiveFormat::Ar,
            "data.tar.gz",
            &opts(),
        )
        .expect("ar over File")
        .source
        .read_bytes()
        .unwrap();

        let ranged = InputSource::File(path).subrange(0, len, "hello.deb");
        let got = extract(&ranged, ArchiveFormat::Ar, "data.tar.gz", &opts())
            .expect("ar over FileRange")
            .source
            .read_bytes()
            .unwrap();

        assert!(!got.is_empty());
        assert_eq!(got, want, "FileRange ar extract must match File extract");
    }

    /// Tar extraction over a `FileRange` source must walk the archive
    /// through the seekable range adapter (`open_seekable`) rather than
    /// buffering the range — covers recursing into a tar carved as a
    /// zero-copy view by a previous extract.
    #[test]
    fn extract_tar_over_file_range_source() {
        let InputSource::File(path) = fixture("archive.tar") else {
            unreachable!("fixture is a File source");
        };
        let len = std::fs::metadata(&path).unwrap().len();
        let src = InputSource::File(path).subrange(0, len, "archive.tar");
        let extracted =
            extract(&src, ArchiveFormat::Tar, STABLE_ENTRY, &opts()).expect("tar over FileRange");
        assert_eq!(extracted.source.read_bytes().unwrap().len(), 2_250);
    }

    /// Compressed-tar extraction over a `TempFile` source streams through
    /// the decoder (no whole-archive read into RAM, no full inflate).
    #[test]
    fn extract_tar_gz_over_tempfile_source() {
        let InputSource::File(path) = fixture("archive.tar.gz") else {
            unreachable!("fixture is a File source");
        };
        let bytes = std::fs::read(&path).unwrap();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut tmp, &bytes).unwrap();
        std::io::Write::flush(&mut tmp).unwrap();
        let src = InputSource::temp_file(tmp, "archive.tar.gz");
        let extracted = extract(&src, ArchiveFormat::TarGz, STABLE_ENTRY, &opts())
            .expect("tar.gz over tempfile");
        assert_eq!(extracted.source.read_bytes().unwrap().len(), 2_250);
    }

    /// Long entry paths (> 100 chars) force a GNU `@LongLink` extension
    /// record ahead of the real header. The zero-copy path uses
    /// `entry.raw_file_position()`, which must point past that extension
    /// to the real file data — otherwise the `FileRange` would slice the
    /// wrong bytes. Guards the realistic deep-path tarball case.
    #[test]
    fn extract_uncompressed_tar_long_path_offset_is_correct() {
        use std::io::Write;
        let long = format!("deeply/nested/{}/leaf.txt", "x".repeat(150));
        let body = b"long path body bytes";

        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, &long, &body[..]).unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&tar_bytes).unwrap();
        tmp.flush().unwrap();
        let src = InputSource::File(tmp.path().to_path_buf());

        let extracted =
            extract(&src, ArchiveFormat::Tar, &long, &opts()).expect("long-path tar extract");
        assert!(
            matches!(extracted.source, InputSource::FileRange { .. }),
            "uncompressed member should be a FileRange, got {:?}",
            extracted.source
        );
        assert_eq!(
            extracted.source.read_bytes().unwrap().as_ref(),
            body,
            "FileRange must slice the real data, not the @LongLink record"
        );
    }

    /// Stored zip over a `FileRange` source: exercises `RangeReadSeek`
    /// under zip's seek-heavy access (EOCD scan from `End`, then seeks to
    /// the local header). Carve a whole on-disk zip as a range and pull a
    /// stored entry back out.
    #[test]
    fn extract_stored_zip_over_file_range_source() {
        use std::io::Write;
        use zip::CompressionMethod;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let payload = b"range-backed stored zip entry";
        let cursor = std::io::Cursor::new(Vec::<u8>::new());
        let mut w = ZipWriter::new(cursor);
        let zopts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        w.start_file("leaf.txt", zopts).unwrap();
        w.write_all(payload).unwrap();
        let zip_bytes = w.finish().unwrap().into_inner();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&zip_bytes).unwrap();
        tmp.flush().unwrap();
        let len = tmp.as_file().metadata().unwrap().len();
        // Whole file carved as a FileRange → open_seekable uses RangeReadSeek.
        let src = InputSource::File(tmp.path().to_path_buf()).subrange(0, len, "a.zip");

        let extracted =
            extract(&src, ArchiveFormat::Zip, "leaf.txt", &opts()).expect("zip over FileRange");
        assert_eq!(extracted.source.read_bytes().unwrap().as_ref(), payload);
    }

    /// Uncompressed tar over a real file: the member is a verbatim slice
    /// of the archive, so extract returns a zero-copy `FileRange` (no
    /// spool, no buffer copy) rather than a `Memory`/`TempFile`.
    #[test]
    fn extract_uncompressed_tar_returns_file_range() {
        let extracted = extract(
            &fixture("archive.tar"),
            ArchiveFormat::Tar,
            STABLE_ENTRY,
            &opts(),
        )
        .expect("tar extract");
        assert!(
            matches!(extracted.source, InputSource::FileRange { .. }),
            "uncompressed tar member should be a zero-copy FileRange, got {:?}",
            extracted.source
        );
        assert_eq!(extracted.source.read_bytes().unwrap().len(), 2_250);
    }

    /// Stored (uncompressed) zip entry over a real file likewise extracts
    /// as a zero-copy `FileRange`. Deflated entries still spool/buffer.
    #[test]
    fn extract_stored_zip_returns_file_range() {
        use std::io::Write;
        use zip::CompressionMethod;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let payload = b"stored entry contents, verbatim on disk";
        let cursor = std::io::Cursor::new(Vec::<u8>::new());
        let mut w = ZipWriter::new(cursor);
        let zopts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        w.start_file("leaf.txt", zopts).unwrap();
        w.write_all(payload).unwrap();
        let zip_bytes = w.finish().unwrap().into_inner();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut tmp, &zip_bytes).unwrap();
        tmp.flush().unwrap();
        let src = InputSource::File(tmp.path().to_path_buf());

        let extracted =
            extract(&src, ArchiveFormat::Zip, "leaf.txt", &opts()).expect("zip extract");
        assert!(
            matches!(extracted.source, InputSource::FileRange { .. }),
            "stored zip entry should be a zero-copy FileRange, got {:?}",
            extracted.source
        );
        assert_eq!(extracted.source.read_bytes().unwrap().as_ref(), payload);
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
            ("archive.tar.br", ArchiveFormat::TarBr),
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

        fn build_zip(entries: &[(&str, &[u8])], method: CompressionMethod) -> Vec<u8> {
            let cursor = std::io::Cursor::new(Vec::<u8>::new());
            let mut w = ZipWriter::new(cursor);
            let opts = SimpleFileOptions::default().compression_method(method);
            for (name, data) in entries {
                w.start_file(*name, opts).unwrap();
                std::io::Write::write_all(&mut w, data).unwrap();
            }
            w.finish().unwrap().into_inner()
        }

        let leaf = b"hello peek recursive".to_vec();
        // Pad inner.zip past SPOOL_THRESHOLD so extracting it from the
        // outer zip lands on the tempfile path. `leaf.txt` is Stored so the
        // recursive extract returns a zero-copy range over the spool.
        let pad = vec![0u8; SPOOL_THRESHOLD as usize];
        let inner_zip = build_zip(
            &[("leaf.txt", &leaf), ("pad.bin", &pad)],
            CompressionMethod::Stored,
        );
        assert!(
            inner_zip.len() as u64 >= SPOOL_THRESHOLD,
            "inner.zip must cross spool threshold"
        );
        // Outer stores nested.zip Deflated so extracting it spools to a
        // TempFile (Stored would yield a zero-copy view and skip the spool
        // path this test exercises).
        let outer_zip = build_zip(&[("nested.zip", &inner_zip)], CompressionMethod::Deflated);

        let outer_src = InputSource::memory(Bytes::from(outer_zip), "outer.zip");
        let first =
            extract(&outer_src, ArchiveFormat::Zip, "nested.zip", &opts()).expect("outer extract");
        assert!(
            matches!(first.source, InputSource::TempFile { .. }),
            "inner.zip should spool, got {:?}",
            first.source
        );

        // Recurse: extract `leaf.txt` from the spooled inner zip. This
        // exercises `open_seekable` on the `TempFile` variant. `leaf.txt`
        // is Stored, so the result is a zero-copy range over the spooled
        // inner zip — carrying the tempfile guard so it outlives `first`.
        let second = extract(&first.source, ArchiveFormat::Zip, "leaf.txt", &opts())
            .expect("nested extract through TempFile source");
        assert_eq!(second.suggested_name, "leaf.txt");
        match &second.source {
            InputSource::FileRange { guard, .. } => {
                assert!(guard.is_some(), "range over a spooled zip must be guarded");
            }
            other => panic!("expected guarded FileRange, got {other:?}"),
        }
        let bytes = second.source.read_bytes().unwrap();
        assert_eq!(bytes.as_ref(), leaf.as_slice());

        // The guard keeps the spool alive even after the source it was
        // carved from drops.
        let leaf_view = second.source;
        drop(first.source);
        assert_eq!(leaf_view.read_bytes().unwrap().as_ref(), leaf.as_slice());
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
