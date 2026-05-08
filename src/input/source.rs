use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bytes::Bytes;

/// Chunk size for streaming line/byte scans of the underlying source.
const SCAN_CHUNK: usize = 64 * 1024;

/// Source of input content.
///
/// `File` reads a path on disk. `Memory` wraps already-buffered bytes
/// (stdin, an extracted in-memory archive entry, an encoded animation
/// frame). `FileRange` is an offset+limit view into a disk file — used
/// when an extractor can map an inner item directly to a byte range of
/// the backing file (ISO entry, uncompressed archive entry) without
/// decompressing or copying.
///
/// `Memory` holds `bytes::Bytes` so cloning the source is a refcount
/// bump rather than a buffer duplication. Sub-slicing in-memory bytes
/// (e.g. extracting an entry from a stdin-piped archive) uses
/// `Bytes::slice` and stays in `Memory`.
///
/// **Invariants for `FileRange`:** `base` always points at a disk file
/// (never an in-memory blob — that case stays in `Memory`). Nested
/// ranges collapse at construction; you should not have a `FileRange`
/// whose `base` ever resolves to another `FileRange`.
#[derive(Clone, Debug)]
pub enum InputSource {
    File(PathBuf),
    Memory {
        bytes: Bytes,
        name: String,
    },
    FileRange {
        base: PathBuf,
        offset: u64,
        len: u64,
        name: String,
    },
}

impl InputSource {
    /// Construct an in-memory source from owned bytes.
    pub fn memory<B: Into<Bytes>>(bytes: B, name: impl Into<String>) -> Self {
        Self::Memory {
            bytes: bytes.into(),
            name: name.into(),
        }
    }

    /// Construct a stdin-backed source. Display name is `<stdin>`.
    pub fn stdin(bytes: impl Into<Bytes>) -> Self {
        Self::Memory {
            bytes: bytes.into(),
            name: "<stdin>".to_string(),
        }
    }

    /// Construct an offset+limit view into a disk file. Used by
    /// extractors that can map their inner item to a byte range of the
    /// backing file without copying.
    pub fn file_range(base: PathBuf, offset: u64, len: u64, name: impl Into<String>) -> Self {
        Self::FileRange {
            base,
            offset,
            len,
            name: name.into(),
        }
    }

    /// Full content as UTF-8 text.
    pub fn read_text(&self) -> Result<String> {
        match self {
            Self::File(path) => fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display())),
            Self::Memory { bytes, name } => std::str::from_utf8(bytes)
                .map(|s| s.to_owned())
                .with_context(|| format!("{name} is not valid UTF-8")),
            Self::FileRange { name, .. } => {
                let raw = self.read_bytes()?;
                String::from_utf8(raw).with_context(|| format!("{name} is not valid UTF-8"))
            }
        }
    }

    /// Full content as raw bytes.
    pub fn read_bytes(&self) -> Result<Vec<u8>> {
        match self {
            Self::File(path) => {
                fs::read(path).with_context(|| format!("failed to read {}", path.display()))
            }
            Self::Memory { bytes, .. } => Ok(bytes.to_vec()),
            Self::FileRange {
                base, offset, len, ..
            } => read_file_range(base, *offset, *len),
        }
    }

    /// Display name: filename for files, stored name for memory/range.
    pub fn name(&self) -> &str {
        match self {
            Self::File(path) => path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            Self::Memory { name, .. } => name,
            Self::FileRange { name, .. } => name,
        }
    }

    /// Filesystem path, when one is meaningful for the user-visible
    /// source. `None` for in-memory and for ranged views (the backing
    /// file path of a range is an internal handle, not the path of the
    /// inner item the user is viewing).
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::File(path) => Some(path.as_path()),
            Self::Memory { .. } => None,
            Self::FileRange { .. } => None,
        }
    }

    /// Convert a 0-based line index to the byte offset where that line
    /// starts. Counts `\n` bytes in the source. Line 0 always starts at 0;
    /// a line index past EOF returns the source length. Returns `None` if
    /// the source can't be read.
    ///
    /// Streams via `open_byte_source` in 64 KB chunks so multi-GB files
    /// don't get loaded into memory.
    ///
    /// For pretty-printed structured content, the displayed line numbers
    /// don't correspond to source line numbers — this conversion is
    /// approximate in that case.
    pub fn line_to_byte(&self, line: usize) -> Option<u64> {
        if line == 0 {
            return Some(0);
        }
        let bs = self.open_byte_source().ok()?;
        let total = bs.len();
        let mut count = 0usize;
        let mut offset: u64 = 0;
        while offset < total {
            let buf = bs.read_range(offset, SCAN_CHUNK).ok()?;
            if buf.is_empty() {
                break;
            }
            for (i, b) in buf.iter().enumerate() {
                if *b == b'\n' {
                    count += 1;
                    if count == line {
                        return Some(offset + (i + 1) as u64);
                    }
                }
            }
            offset += buf.len() as u64;
        }
        Some(total)
    }

    /// Convert a byte offset to a 0-based line index by counting `\n`
    /// bytes up to (but not including) the offset. Offset past EOF
    /// counts the total newlines in the source. Returns `None` if the
    /// source can't be read.
    ///
    /// Streams via `open_byte_source` in 64 KB chunks.
    pub fn byte_to_line(&self, byte: u64) -> Option<usize> {
        let bs = self.open_byte_source().ok()?;
        let limit = byte.min(bs.len());
        let mut count = 0usize;
        let mut offset: u64 = 0;
        while offset < limit {
            let want = ((limit - offset) as usize).min(SCAN_CHUNK);
            let buf = bs.read_range(offset, want).ok()?;
            if buf.is_empty() {
                break;
            }
            count += buf.iter().filter(|b| **b == b'\n').count();
            offset += buf.len() as u64;
        }
        Some(count)
    }

    /// Open a streaming line reader over this source. See `LineSource`
    /// for semantics. Performs one full pass of the source to count lines
    /// and capture sparse byte-offset anchors; subsequent line lookups
    /// are bounded by the anchor stride.
    pub fn open_line_source(&self) -> Result<crate::input::LineSource> {
        crate::input::LineSource::open(self)
    }

    /// Open a streaming byte reader. For files, holds the file handle
    /// and seeks per read. For in-memory sources, shares the buffered
    /// `Bytes` (zero-copy). For ranges, wraps a file reader with offset
    /// translation.
    pub fn open_byte_source(&self) -> Result<Box<dyn ByteSource>> {
        match self {
            Self::File(path) => Ok(Box::new(FileByteSource::open(path)?)),
            Self::Memory { bytes, .. } => Ok(Box::new(BytesByteSource::new(bytes.clone()))),
            Self::FileRange {
                base, offset, len, ..
            } => {
                let f = FileByteSource::open(base)?;
                Ok(Box::new(RangeByteSource::new(Box::new(f), *offset, *len)))
            }
        }
    }
}

fn read_file_range(base: &Path, offset: u64, len: u64) -> Result<Vec<u8>> {
    let mut f = File::open(base).with_context(|| format!("failed to open {}", base.display()))?;
    f.seek(SeekFrom::Start(offset))
        .with_context(|| format!("failed to seek in {}", base.display()))?;
    let cap = usize::try_from(len).unwrap_or(usize::MAX);
    let mut buf = vec![0u8; cap];
    let mut filled = 0usize;
    while filled < cap {
        match f.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) => {
                return Err(anyhow::Error::from(e))
                    .with_context(|| format!("failed to read {}", base.display()));
            }
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

/// Random-access byte reader. Implementations may seek (`File`) or slice
/// (in-memory). Used by the hex viewer to scan a file without loading it
/// fully into memory.
pub trait ByteSource {
    /// Total length of the underlying content in bytes.
    fn len(&self) -> u64;

    /// Read up to `len` bytes starting at `offset`. Returned `Vec` is shorter
    /// than requested only at EOF; reading at or past EOF returns empty.
    fn read_range(&self, offset: u64, len: usize) -> Result<Vec<u8>>;
}

pub struct FileByteSource {
    file: RefCell<File>,
    len: u64,
    path: PathBuf,
}

impl FileByteSource {
    pub fn open(path: &Path) -> Result<Self> {
        let file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let len = file
            .metadata()
            .with_context(|| format!("failed to stat {}", path.display()))?
            .len();
        Ok(Self {
            file: RefCell::new(file),
            len,
            path: path.to_path_buf(),
        })
    }
}

impl ByteSource for FileByteSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_range(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        if offset >= self.len || len == 0 {
            return Ok(Vec::new());
        }
        let remaining = (self.len - offset) as usize;
        let to_read = len.min(remaining);
        let mut buf = vec![0u8; to_read];
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(offset))
            .with_context(|| format!("failed to seek in {}", self.path.display()))?;
        let mut filled = 0;
        while filled < to_read {
            match file.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) => {
                    return Err(anyhow::Error::from(e))
                        .with_context(|| format!("failed to read {}", self.path.display()));
                }
            }
        }
        buf.truncate(filled);
        Ok(buf)
    }
}

pub struct BytesByteSource {
    bytes: Bytes,
}

impl BytesByteSource {
    pub fn new(bytes: Bytes) -> Self {
        Self { bytes }
    }
}

impl ByteSource for BytesByteSource {
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn read_range(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let total = self.bytes.len() as u64;
        if offset >= total || len == 0 {
            return Ok(Vec::new());
        }
        let start = offset as usize;
        let end = (start + len).min(self.bytes.len());
        Ok(self.bytes[start..end].to_vec())
    }
}

/// Offset-and-limit view over another `ByteSource`. Reads translate
/// `0..len` on the view to `base_offset..base_offset+len` on the
/// underlying source. Used by `InputSource::FileRange` to expose an
/// inner item (e.g. an ISO entry) as if it were a standalone source.
pub struct RangeByteSource {
    base: Box<dyn ByteSource>,
    base_offset: u64,
    len: u64,
}

impl RangeByteSource {
    pub fn new(base: Box<dyn ByteSource>, base_offset: u64, len: u64) -> Self {
        // Clamp to the underlying length so downstream code can trust
        // the reported length even if the caller passed a too-large len.
        let total = base.len();
        let start = base_offset.min(total);
        let len = len.min(total.saturating_sub(start));
        Self {
            base,
            base_offset: start,
            len,
        }
    }
}

impl ByteSource for RangeByteSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_range(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        if offset >= self.len || len == 0 {
            return Ok(Vec::new());
        }
        let remaining = self.len - offset;
        let want = (len as u64).min(remaining) as usize;
        self.base.read_range(self.base_offset + offset, want)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, data: &[u8]) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("peek-bytesource-{}-{}", std::process::id(), name));
        let mut f = File::create(&path).unwrap();
        f.write_all(data).unwrap();
        path
    }

    #[test]
    fn file_byte_source_full_read() {
        let path = write_temp("full", b"abcdefghij");
        let bs = FileByteSource::open(&path).unwrap();
        assert_eq!(bs.len(), 10);
        assert_eq!(bs.read_range(0, 10).unwrap(), b"abcdefghij");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn file_byte_source_partial_eof() {
        let path = write_temp("partial", b"abcdefghij");
        let bs = FileByteSource::open(&path).unwrap();
        assert_eq!(bs.read_range(7, 100).unwrap(), b"hij");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn file_byte_source_offset_past_eof() {
        let path = write_temp("past", b"abc");
        let bs = FileByteSource::open(&path).unwrap();
        assert_eq!(bs.read_range(100, 10).unwrap(), Vec::<u8>::new());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn file_byte_source_empty_file() {
        let path = write_temp("empty", b"");
        let bs = FileByteSource::open(&path).unwrap();
        assert_eq!(bs.len(), 0);
        assert_eq!(bs.read_range(0, 10).unwrap(), Vec::<u8>::new());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn file_byte_source_repeated_seeks() {
        let path = write_temp("seek", b"0123456789");
        let bs = FileByteSource::open(&path).unwrap();
        assert_eq!(bs.read_range(0, 3).unwrap(), b"012");
        assert_eq!(bs.read_range(7, 3).unwrap(), b"789");
        assert_eq!(bs.read_range(3, 4).unwrap(), b"3456");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn bytes_byte_source_full_read() {
        let bs = BytesByteSource::new(Bytes::from_static(b"abcdefghij"));
        assert_eq!(bs.len(), 10);
        assert_eq!(bs.read_range(0, 10).unwrap(), b"abcdefghij");
    }

    #[test]
    fn bytes_byte_source_partial_eof() {
        let bs = BytesByteSource::new(Bytes::from_static(b"abcdefghij"));
        assert_eq!(bs.read_range(7, 100).unwrap(), b"hij");
    }

    #[test]
    fn bytes_byte_source_offset_past_eof() {
        let bs = BytesByteSource::new(Bytes::from_static(b"abc"));
        assert_eq!(bs.read_range(100, 10).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn bytes_byte_source_empty() {
        let bs = BytesByteSource::new(Bytes::new());
        assert_eq!(bs.len(), 0);
        assert_eq!(bs.read_range(0, 10).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn range_byte_source_translates_offsets() {
        let inner = BytesByteSource::new(Bytes::from_static(b"0123456789"));
        let bs = RangeByteSource::new(Box::new(inner), 3, 5);
        assert_eq!(bs.len(), 5);
        assert_eq!(bs.read_range(0, 5).unwrap(), b"34567");
        assert_eq!(bs.read_range(2, 2).unwrap(), b"56");
        // Read past view EOF returns empty even though base has more.
        assert_eq!(bs.read_range(100, 10).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn range_byte_source_clamps_oversized_len() {
        let inner = BytesByteSource::new(Bytes::from_static(b"abcdef"));
        let bs = RangeByteSource::new(Box::new(inner), 4, 100);
        assert_eq!(bs.len(), 2, "clamped to remaining bytes");
        assert_eq!(bs.read_range(0, 10).unwrap(), b"ef");
    }

    #[test]
    fn input_source_file_range_round_trip() {
        let path = write_temp("range", b"AAAAhelloBBBB");
        let src = InputSource::file_range(path.clone(), 4, 5, "hello".to_string());
        assert_eq!(src.read_bytes().unwrap(), b"hello");
        assert_eq!(src.read_text().unwrap(), "hello");
        assert_eq!(src.name(), "hello");
        assert!(src.path().is_none());
        let bs = src.open_byte_source().unwrap();
        assert_eq!(bs.len(), 5);
        assert_eq!(bs.read_range(1, 3).unwrap(), b"ell");
        let _ = fs::remove_file(&path);
    }

    fn stdin_source(text: &str) -> InputSource {
        InputSource::stdin(Bytes::copy_from_slice(text.as_bytes()))
    }

    #[test]
    fn line_to_byte_first_line_is_zero() {
        let s = stdin_source("alpha\nbeta\ngamma\n");
        assert_eq!(s.line_to_byte(0), Some(0));
    }

    #[test]
    fn line_to_byte_after_n_newlines() {
        let s = stdin_source("alpha\nbeta\ngamma\n");
        // line 1 starts at byte 6 (after "alpha\n")
        assert_eq!(s.line_to_byte(1), Some(6));
        // line 2 starts at byte 11 (after "alpha\nbeta\n")
        assert_eq!(s.line_to_byte(2), Some(11));
    }

    #[test]
    fn line_to_byte_past_eof_returns_len() {
        let s = stdin_source("a\nb\nc\n");
        let len = "a\nb\nc\n".len() as u64;
        assert_eq!(s.line_to_byte(999), Some(len));
    }

    #[test]
    fn line_to_byte_no_trailing_newline() {
        let s = stdin_source("first\nsecond");
        assert_eq!(s.line_to_byte(1), Some(6));
        // line 2 doesn't exist (only one newline) → returns len
        let len = "first\nsecond".len() as u64;
        assert_eq!(s.line_to_byte(2), Some(len));
    }

    #[test]
    fn byte_to_line_round_trips_with_line_to_byte() {
        let s = stdin_source("alpha\nbeta\ngamma\ndelta\n");
        for line in 0..4 {
            let byte = s.line_to_byte(line).unwrap();
            assert_eq!(
                s.byte_to_line(byte),
                Some(line),
                "round trip failed at line {line}"
            );
        }
    }

    #[test]
    fn byte_to_line_past_eof_counts_total_newlines() {
        let s = stdin_source("a\nb\nc\n");
        // 3 newlines in the source; offset past end should report all of them.
        assert_eq!(s.byte_to_line(999), Some(3));
    }
}
