use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Source of input content — either a file on disk or buffered stdin.
///
/// Decouples "where data comes from" from "how it's displayed".
#[derive(Clone)]
pub enum InputSource {
    File(PathBuf),
    Stdin { data: Vec<u8> },
}

impl InputSource {
    /// Full content as UTF-8 text.
    pub fn read_text(&self) -> Result<String> {
        match self {
            Self::File(path) => fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display())),
            Self::Stdin { data } => {
                String::from_utf8(data.clone()).context("stdin is not valid UTF-8")
            }
        }
    }

    /// Full content as raw bytes.
    pub fn read_bytes(&self) -> Result<Vec<u8>> {
        match self {
            Self::File(path) => {
                fs::read(path).with_context(|| format!("failed to read {}", path.display()))
            }
            Self::Stdin { data } => Ok(data.clone()),
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

    /// Open a streaming byte reader. For files, holds the file handle and
    /// seeks per read. For stdin, slices the already-buffered bytes.
    pub fn open_byte_source(&self) -> Result<Box<dyn ByteSource>> {
        match self {
            Self::File(path) => Ok(Box::new(FileByteSource::open(path)?)),
            Self::Stdin { data } => Ok(Box::new(SliceByteSource::new(data.clone()))),
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
    data: Vec<u8>,
}

impl SliceByteSource {
    pub fn new(data: Vec<u8>) -> Self {
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

    #[test]
    fn slice_byte_source_full_read() {
        let bs = SliceByteSource::new(b"abcdefghij".to_vec());
        assert_eq!(bs.len(), 10);
        assert_eq!(bs.read_range(0, 10).unwrap(), b"abcdefghij");
    }

    #[test]
    fn slice_byte_source_partial_eof() {
        let bs = SliceByteSource::new(b"abcdefghij".to_vec());
        assert_eq!(bs.read_range(7, 100).unwrap(), b"hij");
    }

    #[test]
    fn slice_byte_source_offset_past_eof() {
        let bs = SliceByteSource::new(b"abc".to_vec());
        assert_eq!(bs.read_range(100, 10).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn slice_byte_source_empty() {
        let bs = SliceByteSource::new(Vec::new());
        assert_eq!(bs.len(), 0);
        assert_eq!(bs.read_range(0, 10).unwrap(), Vec::<u8>::new());
    }
}
