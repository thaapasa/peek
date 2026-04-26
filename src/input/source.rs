use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};

/// Chunk size for streaming line/byte scans of the underlying source.
const SCAN_CHUNK: usize = 64 * 1024;

/// Source of input content — either a file on disk or buffered stdin.
///
/// Decouples "where data comes from" from "how it's displayed".
///
/// Stdin holds an `Arc<[u8]>` so cloning the source — which happens once
/// per mode in the stack and at every `open_byte_source` call — is a
/// pointer copy rather than a buffer duplication.
#[derive(Clone)]
pub enum InputSource {
    File(PathBuf),
    Stdin { data: Arc<[u8]> },
}

impl InputSource {
    /// Full content as UTF-8 text.
    pub fn read_text(&self) -> Result<String> {
        match self {
            Self::File(path) => fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display())),
            Self::Stdin { data } => std::str::from_utf8(data)
                .map(|s| s.to_owned())
                .context("stdin is not valid UTF-8"),
        }
    }

    /// Full content as raw bytes.
    pub fn read_bytes(&self) -> Result<Vec<u8>> {
        match self {
            Self::File(path) => {
                fs::read(path).with_context(|| format!("failed to read {}", path.display()))
            }
            Self::Stdin { data } => Ok(data.to_vec()),
        }
    }

    /// Display name: filename for files, `<stdin>` for stdin.
    pub fn name(&self) -> &str {
        match self {
            Self::File(path) => path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            Self::Stdin { .. } => "<stdin>",
        }
    }

    /// Filesystem path (None for stdin).
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::File(path) => Some(path.as_path()),
            Self::Stdin { .. } => None,
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

    /// Open a streaming byte reader. For files, holds the file handle and
    /// seeks per read. For stdin, shares the already-buffered bytes via Arc.
    pub fn open_byte_source(&self) -> Result<Box<dyn ByteSource>> {
        match self {
            Self::File(path) => Ok(Box::new(FileByteSource::open(path)?)),
            Self::Stdin { data } => Ok(Box::new(SliceByteSource::new(Arc::clone(data)))),
        }
    }
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

pub struct SliceByteSource {
    data: Arc<[u8]>,
}

impl SliceByteSource {
    pub fn new(data: Arc<[u8]>) -> Self {
        Self { data }
    }
}

impl ByteSource for SliceByteSource {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn read_range(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let total = self.data.len() as u64;
        if offset >= total || len == 0 {
            return Ok(Vec::new());
        }
        let start = offset as usize;
        let end = (start + len).min(self.data.len());
        Ok(self.data[start..end].to_vec())
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

    fn arc_bytes(b: &[u8]) -> Arc<[u8]> {
        Arc::from(b.to_vec().into_boxed_slice())
    }

    #[test]
    fn slice_byte_source_full_read() {
        let bs = SliceByteSource::new(arc_bytes(b"abcdefghij"));
        assert_eq!(bs.len(), 10);
        assert_eq!(bs.read_range(0, 10).unwrap(), b"abcdefghij");
    }

    #[test]
    fn slice_byte_source_partial_eof() {
        let bs = SliceByteSource::new(arc_bytes(b"abcdefghij"));
        assert_eq!(bs.read_range(7, 100).unwrap(), b"hij");
    }

    #[test]
    fn slice_byte_source_offset_past_eof() {
        let bs = SliceByteSource::new(arc_bytes(b"abc"));
        assert_eq!(bs.read_range(100, 10).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn slice_byte_source_empty() {
        let bs = SliceByteSource::new(arc_bytes(&[]));
        assert_eq!(bs.len(), 0);
        assert_eq!(bs.read_range(0, 10).unwrap(), Vec::<u8>::new());
    }

    fn stdin_source(text: &str) -> InputSource {
        InputSource::Stdin {
            data: arc_bytes(text.as_bytes()),
        }
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
            assert_eq!(s.byte_to_line(byte), Some(line), "round trip failed at line {line}");
        }
    }

    #[test]
    fn byte_to_line_past_eof_counts_total_newlines() {
        let s = stdin_source("a\nb\nc\n");
        // 3 newlines in the source; offset past end should report all of them.
        assert_eq!(s.byte_to_line(999), Some(3));
    }
}
