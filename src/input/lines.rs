use std::ops::Range;

use anyhow::{Context, Result};

use super::source::{ByteSource, InputSource};

/// Chunk size for streamed reads. Matches the value `InputSource` uses for
/// its line/byte scans so behavior is consistent across the input layer.
const READ_CHUNK: usize = 64 * 1024;

/// Capture a byte-offset anchor every N lines so jumping into the middle
/// of a multi-million-line file only requires reading at most N lines from
/// the nearest anchor.
const ANCHOR_STRIDE: usize = 1024;

/// Streaming, random-access view of an input source as a sequence of
/// lines. Total line count and a sparse byte-offset anchor table are
/// computed once at construction (one full pass over the source); after
/// that, individual lines or windows can be fetched without ever loading
/// the whole file into memory.
///
/// Line semantics match `str::lines`:
/// - Lines are split on `\n`; a trailing `\r` is stripped.
/// - A trailing fragment without a `\n` counts as its own line.
/// - An empty source has zero lines; `"\n"` has one (empty) line.
///
/// Stdin and file sources go through the same path: stdin is already
/// in-memory (`Arc<[u8]>`), so the "streaming" reads are zero-cost slices,
/// and files seek per chunk via `FileByteSource`.
pub struct LineSource {
    bs: Box<dyn ByteSource>,
    /// `anchors[k]` is the byte offset where line `k * ANCHOR_STRIDE` starts.
    /// Always non-empty: `anchors[0]` is `0`.
    anchors: Vec<u64>,
    total_lines: usize,
    total_bytes: u64,
}

impl LineSource {
    /// Build a `LineSource` over the given input. Performs one streaming
    /// scan of the source to count newlines and capture sparse anchors.
    pub fn open(source: &InputSource) -> Result<Self> {
        let bs = source.open_byte_source()?;
        let (anchors, total_lines, total_bytes) = scan(bs.as_ref())?;
        Ok(Self {
            bs,
            anchors,
            total_lines,
            total_bytes,
        })
    }

    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Fetch the line at `idx` (0-based). Returns an empty string for
    /// indices at or past `total_lines`.
    pub fn line(&self, idx: usize) -> Result<String> {
        let mut v = self.window(idx..idx + 1)?;
        Ok(v.pop().unwrap_or_default())
    }

    /// Fetch lines in the half-open range `[start, end)`. Out-of-range
    /// indices are clamped to `total_lines` (so a caller can pass
    /// `scroll..scroll+rows` and get fewer lines back near EOF).
    pub fn window(&self, range: Range<usize>) -> Result<Vec<String>> {
        let start = range.start.min(self.total_lines);
        let end = range.end.min(self.total_lines);
        if start >= end {
            return Ok(Vec::new());
        }

        let anchor_idx = start / ANCHOR_STRIDE;
        let anchor_line = anchor_idx * ANCHOR_STRIDE;
        let anchor_byte = self.anchors[anchor_idx];

        let mut reader = LineReader::new(self.bs.as_ref(), anchor_byte);

        // Skip lines between the anchor and the window start.
        for _ in anchor_line..start {
            if reader.next_line()?.is_none() {
                return Ok(Vec::new());
            }
        }

        let mut out = Vec::with_capacity(end - start);
        for _ in start..end {
            match reader.next_line()? {
                Some(line) => out.push(line),
                None => break,
            }
        }
        Ok(out)
    }

    /// Iterate every line from the start. Callers like the pipe path
    /// stream the whole file through this; callers needing random access
    /// use `window`.
    pub fn iter_all(&self) -> LineReader<'_> {
        LineReader::new(self.bs.as_ref(), 0)
    }
}

/// Forward-only line iterator over a `ByteSource`. Pulls chunks lazily
/// and accumulates the in-progress line in a small carry buffer so chunk
/// boundaries never split a line.
pub struct LineReader<'a> {
    bs: &'a dyn ByteSource,
    /// Next byte offset to read from the source.
    next_offset: u64,
    /// Accumulated bytes of the in-progress line, before its `\n`.
    carry: Vec<u8>,
    /// True once the source has been fully read.
    eof: bool,
    /// True once the synthesized trailing line (file with no final `\n`)
    /// has been emitted.
    final_emitted: bool,
}

impl<'a> LineReader<'a> {
    fn new(bs: &'a dyn ByteSource, start: u64) -> Self {
        Self {
            bs,
            next_offset: start,
            carry: Vec::new(),
            eof: false,
            final_emitted: false,
        }
    }

    /// Read the next complete line. Returns `Ok(None)` after EOF (and
    /// after any trailing no-newline fragment has been emitted).
    pub fn next_line(&mut self) -> Result<Option<String>> {
        loop {
            if let Some(pos) = self.carry.iter().position(|b| *b == b'\n') {
                let mut line_bytes = self.carry.split_off(pos + 1);
                std::mem::swap(&mut line_bytes, &mut self.carry);
                line_bytes.pop(); // drop the '\n'
                if line_bytes.last() == Some(&b'\r') {
                    line_bytes.pop();
                }
                return Ok(Some(decode(line_bytes)?));
            }

            if self.eof {
                if !self.final_emitted && !self.carry.is_empty() {
                    self.final_emitted = true;
                    let bytes = std::mem::take(&mut self.carry);
                    return Ok(Some(decode(bytes)?));
                }
                return Ok(None);
            }

            let buf = self.bs.read_range(self.next_offset, READ_CHUNK)?;
            if buf.is_empty() {
                self.eof = true;
                continue;
            }
            self.next_offset += buf.len() as u64;
            self.carry.extend_from_slice(&buf);
        }
    }
}

impl Iterator for LineReader<'_> {
    type Item = Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_line() {
            Ok(Some(s)) => Some(Ok(s)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

fn decode(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).context("input is not valid UTF-8")
}

/// One streaming pass: count newlines, capture an anchor every
/// `ANCHOR_STRIDE` lines, return the totals.
///
/// Newline-byte counting is safe over raw bytes: in UTF-8, no continuation
/// or multibyte-start byte ever has the value `0x0A`, so the count is
/// independent of how chunks split codepoints.
fn scan(bs: &dyn ByteSource) -> Result<(Vec<u64>, usize, u64)> {
    let total_bytes = bs.len();
    let mut anchors = vec![0u64];
    let mut newline_count = 0usize;
    let mut offset = 0u64;
    let mut last_byte: Option<u8> = None;

    while offset < total_bytes {
        let buf = bs.read_range(offset, READ_CHUNK)?;
        if buf.is_empty() {
            break;
        }
        for (i, b) in buf.iter().enumerate() {
            if *b == b'\n' {
                newline_count += 1;
                if newline_count.is_multiple_of(ANCHOR_STRIDE) {
                    anchors.push(offset + (i + 1) as u64);
                }
            }
        }
        last_byte = buf.last().copied().or(last_byte);
        offset += buf.len() as u64;
    }

    let total_lines = newline_count
        + match last_byte {
            Some(b'\n') => 0,
            Some(_) => 1,
            None => 0,
        };

    Ok((anchors, total_lines, total_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn stdin_source(text: &str) -> InputSource {
        InputSource::Stdin {
            data: Arc::from(text.as_bytes().to_vec().into_boxed_slice()),
        }
    }

    fn write_temp(name: &str, data: &[u8]) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("peek-linesource-{}-{}", std::process::id(), name));
        let mut f = File::create(&path).unwrap();
        f.write_all(data).unwrap();
        path
    }

    #[test]
    fn empty_source_has_zero_lines() {
        let s = stdin_source("");
        let ls = LineSource::open(&s).unwrap();
        assert_eq!(ls.total_lines(), 0);
        assert_eq!(ls.window(0..10).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn single_line_no_trailing_newline() {
        let s = stdin_source("only");
        let ls = LineSource::open(&s).unwrap();
        assert_eq!(ls.total_lines(), 1);
        assert_eq!(ls.window(0..1).unwrap(), vec!["only"]);
    }

    #[test]
    fn single_line_with_trailing_newline() {
        let s = stdin_source("only\n");
        let ls = LineSource::open(&s).unwrap();
        assert_eq!(ls.total_lines(), 1);
        assert_eq!(ls.window(0..1).unwrap(), vec!["only"]);
    }

    #[test]
    fn multiple_lines_with_and_without_tail() {
        let with_tail = stdin_source("a\nb\nc\n");
        assert_eq!(LineSource::open(&with_tail).unwrap().total_lines(), 3);

        let no_tail = stdin_source("a\nb\nc");
        assert_eq!(LineSource::open(&no_tail).unwrap().total_lines(), 3);
    }

    #[test]
    fn empty_lines_count() {
        // matches str::lines("a\n\nb") = ["a", "", "b"]
        let s = stdin_source("a\n\nb\n");
        let ls = LineSource::open(&s).unwrap();
        assert_eq!(ls.total_lines(), 3);
        assert_eq!(ls.window(0..3).unwrap(), vec!["a", "", "b"]);
    }

    #[test]
    fn crlf_line_endings_are_stripped() {
        let s = stdin_source("alpha\r\nbeta\r\ngamma");
        let ls = LineSource::open(&s).unwrap();
        assert_eq!(ls.total_lines(), 3);
        assert_eq!(ls.window(0..3).unwrap(), vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn matches_str_lines_for_common_inputs() {
        for input in ["", "x", "x\n", "x\ny", "x\ny\n", "\n", "\n\n", "a\n\nb"] {
            let s = stdin_source(input);
            let ls = LineSource::open(&s).unwrap();
            let expected: Vec<String> = input.lines().map(String::from).collect();
            assert_eq!(ls.total_lines(), expected.len(), "input = {input:?}");
            assert_eq!(ls.window(0..expected.len()).unwrap(), expected);
        }
    }

    #[test]
    fn window_in_middle_of_file() {
        let mut text = String::new();
        for i in 0..200 {
            text.push_str(&format!("line {i}\n"));
        }
        let s = stdin_source(&text);
        let ls = LineSource::open(&s).unwrap();
        assert_eq!(ls.total_lines(), 200);
        let w = ls.window(50..55).unwrap();
        assert_eq!(
            w,
            vec!["line 50", "line 51", "line 52", "line 53", "line 54"]
        );
    }

    #[test]
    fn window_clamps_past_eof() {
        let s = stdin_source("a\nb\nc\n");
        let ls = LineSource::open(&s).unwrap();
        assert_eq!(ls.window(2..10).unwrap(), vec!["c"]);
        assert_eq!(ls.window(10..20).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn line_lookup_round_trips_with_window() {
        let s = stdin_source("alpha\nbeta\ngamma\ndelta\n");
        let ls = LineSource::open(&s).unwrap();
        for i in 0..4 {
            assert_eq!(ls.line(i).unwrap(), ls.window(i..i + 1).unwrap()[0]);
        }
    }

    #[test]
    fn anchors_capture_periodic_offsets() {
        // Build a source with 3 anchor strides + a remainder so we exercise
        // both the anchor jump and the post-anchor skip.
        let n = ANCHOR_STRIDE * 3 + 17;
        let mut text = String::with_capacity(n * 8);
        for i in 0..n {
            text.push_str(&format!("L{i}\n"));
        }
        let s = stdin_source(&text);
        let ls = LineSource::open(&s).unwrap();
        assert_eq!(ls.total_lines(), n);
        // Jump deep into the file via window — verifies anchor lookup +
        // skip path produce the right line.
        let target = ANCHOR_STRIDE * 2 + 5;
        let w = ls.window(target..target + 1).unwrap();
        assert_eq!(w, vec![format!("L{target}")]);
    }

    #[test]
    fn multibyte_utf8_across_chunk_boundary() {
        // Pad the source so a multibyte character straddles a 64 KB
        // read boundary. The newline-byte counter operates on bytes
        // (safe for UTF-8) and the line decoder must reassemble the
        // codepoint across chunks.
        let pad = "x".repeat(READ_CHUNK - 1); // boundary lands inside "ä"
        let text = format!("{pad}ä\nnext\n");
        let s = stdin_source(&text);
        let ls = LineSource::open(&s).unwrap();
        assert_eq!(ls.total_lines(), 2);
        assert_eq!(
            ls.window(0..2).unwrap(),
            vec![format!("{pad}ä"), "next".to_string()]
        );
    }

    #[test]
    fn file_backed_source_matches_stdin() {
        let text = "alpha\nbeta\ngamma\ndelta\n";
        let path = write_temp("file-match", text.as_bytes());
        let file_src = InputSource::File(path.clone());
        let stdin_src = stdin_source(text);

        let from_file = LineSource::open(&file_src).unwrap();
        let from_stdin = LineSource::open(&stdin_src).unwrap();

        assert_eq!(from_file.total_lines(), from_stdin.total_lines());
        assert_eq!(
            from_file.window(0..from_file.total_lines()).unwrap(),
            from_stdin.window(0..from_stdin.total_lines()).unwrap()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn iter_all_yields_every_line() {
        let s = stdin_source("a\nb\nc\nd");
        let ls = LineSource::open(&s).unwrap();
        let collected: Vec<String> = ls.iter_all().collect::<Result<_>>().unwrap();
        assert_eq!(collected, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn invalid_utf8_errors() {
        // Lone continuation byte; not valid UTF-8.
        let s = InputSource::Stdin {
            data: Arc::from(vec![0x80].into_boxed_slice()),
        };
        let ls = LineSource::open(&s).unwrap();
        assert!(ls.line(0).is_err());
    }
}
