//! Sequential byte stream over a sub-range of a [`ByteSource`].
//!
//! Wraps any `ByteSource` and exposes it as both [`io::Read`] and
//! [`io::BufRead`] so callers can use `io::copy`, `read_until`, `lines`,
//! `take`, and the rest of the `std::io` ecosystem instead of pumping
//! `read_range` in a loop.
//!
//! Internally pulls fixed-size chunks (default 64 KiB) from the underlying
//! source via `read_range`. `FileByteSource` caches the current file
//! position so a sequential walk costs one seek total; `BytesByteSource`
//! reads are pure slicing.
//!
//! The stream owns its chunk buffer and exposes it directly via `BufRead`,
//! so callers do **not** need to wrap it in `BufReader` — that would
//! triple-buffer (source → stream → BufReader). Use the stream's own
//! `BufRead` methods.

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

impl io::BufRead for ByteStream {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.buf.is_empty() && self.offset < self.end {
            let want = ((self.end - self.offset) as usize).min(DEFAULT_CHUNK);
            let chunk = self
                .bs
                .read_range(self.offset, want)
                .map_err(io::Error::other)?;
            self.offset += chunk.len() as u64;
            self.buf = chunk;
        }
        Ok(&self.buf)
    }

    fn consume(&mut self, amt: usize) {
        let n = amt.min(self.buf.len());
        self.buf = self.buf.slice(n..);
    }
}

impl io::Read for ByteStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let buf = io::BufRead::fill_buf(self)?;
        let n = buf.len().min(out.len());
        out[..n].copy_from_slice(&buf[..n]);
        io::BufRead::consume(self, n);
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

    #[test]
    fn bufread_read_until_yields_each_line() {
        use std::io::BufRead;
        let mut s = ByteStream::open(source(b"alpha\nbeta\ngamma\n"));
        let mut lines = Vec::new();
        loop {
            let mut buf = Vec::new();
            let n = s.read_until(b'\n', &mut buf).unwrap();
            if n == 0 {
                break;
            }
            lines.push(buf);
        }
        assert_eq!(
            lines,
            vec![b"alpha\n".to_vec(), b"beta\n".to_vec(), b"gamma\n".to_vec()]
        );
    }

    #[test]
    fn bufread_fill_buf_then_consume() {
        use std::io::BufRead;
        let mut s = ByteStream::open(source(b"abcdef"));
        let buf = s.fill_buf().unwrap().to_vec();
        assert_eq!(buf, b"abcdef");
        s.consume(3);
        let rest = s.fill_buf().unwrap().to_vec();
        assert_eq!(rest, b"def");
    }
}
