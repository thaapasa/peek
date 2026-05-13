//! Sequential byte stream over a sub-range of a [`ByteSource`].
//!
//! Wraps any `ByteSource` and exposes it as an [`io::Read`] so callers
//! can use `io::copy`, `BufReader`, `take`, and the rest of the
//! `std::io` ecosystem instead of pumping `read_range` in a loop.
//!
//! Internally pulls fixed-size chunks (default 64 KiB) from the underlying
//! source via `read_range`. `FileByteSource` caches the current file
//! position so a sequential walk costs one seek total; `BytesByteSource`
//! reads are pure slicing.

use std::io;

use bytes::Bytes;

use super::source::ByteSource;

/// Default chunk size pulled from the underlying `ByteSource` on each
/// refill (64 KiB).
pub const DEFAULT_CHUNK: usize = 64 * 1024;

/// Sequential byte stream over a sub-range of a `ByteSource`. Implements
/// `io::Read`. `len()` reports the total bytes the stream will yield
/// before EOF.
pub struct ByteStream {
    bs: Box<dyn ByteSource>,
    /// Next absolute offset to pull from the underlying source.
    offset: u64,
    /// Absolute offset one past the last byte the stream is allowed to yield.
    end: u64,
    /// Most-recent chunk pulled from `bs`; drained into the caller's
    /// buffer before the next `read_range` call.
    buf: Bytes,
}

impl ByteStream {
    /// Stream the entire underlying source.
    pub fn open(bs: Box<dyn ByteSource>) -> Self {
        let end = bs.len();
        Self::range(bs, 0, end)
    }

    /// Stream a sub-range `[offset, offset+len)`. Both ends are clamped
    /// to the source's length.
    pub fn range(bs: Box<dyn ByteSource>, offset: u64, len: u64) -> Self {
        let source_len = bs.len();
        let start = offset.min(source_len);
        let end = start.saturating_add(len.min(source_len.saturating_sub(start)));
        Self {
            bs,
            offset: start,
            end,
            buf: Bytes::new(),
        }
    }
}

impl io::Read for ByteStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.buf.is_empty() {
            if self.offset >= self.end {
                return Ok(0);
            }
            let want = ((self.end - self.offset) as usize).min(DEFAULT_CHUNK);
            let chunk = self
                .bs
                .read_range(self.offset, want)
                .map_err(io::Error::other)?;
            if chunk.is_empty() {
                return Ok(0);
            }
            self.offset += chunk.len() as u64;
            self.buf = chunk;
        }
        let n = self.buf.len().min(out.len());
        out[..n].copy_from_slice(&self.buf[..n]);
        self.buf = self.buf.slice(n..);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::source::BytesByteSource;
    use bytes::Bytes;
    use std::io::Read;

    fn source(data: &'static [u8]) -> Box<dyn ByteSource> {
        Box::new(BytesByteSource::new(Bytes::from_static(data)))
    }

    #[test]
    fn open_streams_full_source() {
        let mut s = ByteStream::open(source(b"hello world"));
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn range_clamps_to_source_len() {
        let mut s = ByteStream::range(source(b"abcdef"), 4, 100);
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"ef");
    }

    #[test]
    fn range_skips_prefix_and_suffix() {
        let mut s = ByteStream::range(source(b"prefix-body-suffix"), 7, 4);
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"body");
    }
}
