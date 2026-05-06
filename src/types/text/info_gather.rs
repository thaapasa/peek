//! Streaming text-stats collection. A single pass over the byte stream
//! computes char/word/line counts, blank-line count, longest-line width,
//! line-ending classification, indent style, and any leading shebang. BOM
//! detection picks an encoding up front; UTF-16 takes a separate
//! whole-file path because the chunked UTF-8 path can't validate split
//! 16-bit code units.

use crate::info::{Encoding, IndentStyle, LineEndings, TextStats};
use crate::input::{ByteSource, InputSource};

/// Chunk size for streaming text-extras counting.
const TEXT_SCAN_CHUNK: usize = 64 * 1024;

/// Stream the source and collect [`TextStats`]. Returns `None` if the
/// content isn't valid UTF-8 (or its UTF-16 BOM-prefixed equivalent) — the
/// caller treats that as a binary file.
pub fn gather_text_stats(source: &InputSource) -> Option<TextStats> {
    let bs = source.open_byte_source().ok()?;
    let total = bs.len();
    if total == 0 {
        return Some(empty_stats());
    }

    // Detect BOM up-front; advance past it for the rest of the scan.
    let head = bs.read_range(0, 4).ok()?;
    let (encoding, offset) = detect_bom(&head);
    if let Some(stats) = decode_utf16_stats(bs.as_ref(), encoding, offset, total) {
        return Some(stats);
    }

    stream_utf8(bs.as_ref(), encoding, offset, total)
}

fn empty_stats() -> TextStats {
    TextStats {
        line_count: 0,
        word_count: 0,
        char_count: 0,
        blank_lines: 0,
        longest_line_chars: 0,
        line_endings: LineEndings::None,
        indent_style: None,
        encoding: Encoding::Utf8,
        shebang: None,
    }
}

fn detect_bom(head: &[u8]) -> (Encoding, u64) {
    if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (Encoding::Utf8Bom, 3);
    }
    if head.starts_with(&[0xFF, 0xFE]) {
        return (Encoding::Utf16Le, 2);
    }
    if head.starts_with(&[0xFE, 0xFF]) {
        return (Encoding::Utf16Be, 2);
    }
    (Encoding::Utf8, 0)
}

// ---------------------------------------------------------------------------
// UTF-8 streaming pass
// ---------------------------------------------------------------------------

/// Walk a UTF-8 source in [`TEXT_SCAN_CHUNK`]-sized pieces, collecting all
/// [`TextStats`] fields. The accumulator state below is what the next
/// chunk needs to continue counting words / line endings / indent
/// classification across boundaries.
fn stream_utf8(
    bs: &dyn ByteSource,
    encoding: Encoding,
    initial_offset: u64,
    total: u64,
) -> Option<TextStats> {
    let mut state = ScanState::new();
    let mut buf: Vec<u8> = Vec::with_capacity(TEXT_SCAN_CHUNK);
    let mut offset = initial_offset;
    let mut at_file_start = offset == 0;
    let mut shebang_bytes: Option<Vec<u8>> = None;

    while offset < total {
        let want = ((total - offset) as usize).min(TEXT_SCAN_CHUNK);
        let read = bs.read_range(offset, want).ok()?;
        if read.is_empty() {
            break;
        }
        offset += read.len() as u64;
        buf.extend_from_slice(&read);

        let valid_up_to = match std::str::from_utf8(&buf) {
            Ok(_) => buf.len(),
            Err(e) => {
                if e.error_len().is_some() {
                    return None;
                }
                e.valid_up_to()
            }
        };
        let s = std::str::from_utf8(&buf[..valid_up_to]).expect("valid_up_to slice is valid UTF-8");

        if at_file_start && s.starts_with("#!") {
            let line_end = s.find('\n').unwrap_or(s.len());
            let mut line: Vec<u8> = s.as_bytes()[..line_end].to_vec();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            shebang_bytes = Some(line);
            at_file_start = false;
        }
        if at_file_start {
            at_file_start = false;
        }

        for ch in s.chars() {
            state.consume(ch);
        }
        buf.drain(..valid_up_to);
    }

    if !buf.is_empty() {
        // Trailing incomplete UTF-8 sequence — treat as binary.
        return None;
    }

    let shebang = shebang_bytes
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| s.trim_start_matches("#!").trim().to_string());
    Some(state.finish(encoding, shebang))
}

// ---------------------------------------------------------------------------
// UTF-16 fallback — small files only, decoded fully
// ---------------------------------------------------------------------------

/// Synchronous text analysis on a fully-loaded string. UTF-16 files in the
/// wild are essentially always small config / script files, so in-memory
/// decoding here is fine — chunked streaming would have to track surrogate
/// pairs across boundaries for marginal gain.
fn decode_utf16_stats(
    bs: &dyn ByteSource,
    encoding: Encoding,
    offset: u64,
    total: u64,
) -> Option<TextStats> {
    if !matches!(encoding, Encoding::Utf16Le | Encoding::Utf16Be) {
        return None;
    }
    let body = bs.read_range(offset, (total - offset) as usize).ok()?;
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|c| match encoding {
            Encoding::Utf16Le => u16::from_le_bytes([c[0], c[1]]),
            Encoding::Utf16Be => u16::from_be_bytes([c[0], c[1]]),
            _ => 0,
        })
        .collect();
    let s = String::from_utf16_lossy(&units);

    let mut state = ScanState::new();
    for ch in s.chars() {
        state.consume(ch);
    }
    let shebang = s
        .strip_prefix("#!")
        .and_then(|rest| rest.lines().next().map(|l| l.trim().to_string()));
    Some(state.finish(encoding, shebang))
}

// ---------------------------------------------------------------------------
// Shared scan accumulator
// ---------------------------------------------------------------------------

/// Per-character accumulator. Keeps running totals plus the cross-character
/// state needed to classify line endings, words, and indent style without
/// re-walking the source.
struct ScanState {
    chars: usize,
    words: usize,
    lf_count: usize,
    crlf_count: usize,
    cr_count: usize,
    blank_lines: usize,
    longest: usize,

    // Current-line tracking
    cur_line_chars: usize,
    cur_line_blank: bool,

    // Indent tracking
    tab_indents: usize,
    space_indents: usize,
    space_widths: [usize; 9],
    at_line_start: bool,
    counting_indent: bool,
    cur_indent_spaces: u8,

    // Cross-character state
    in_word: bool,
    prev_was_cr: bool,
    last_char: Option<char>,
}

impl ScanState {
    fn new() -> Self {
        Self {
            chars: 0,
            words: 0,
            lf_count: 0,
            crlf_count: 0,
            cr_count: 0,
            blank_lines: 0,
            longest: 0,
            cur_line_chars: 0,
            cur_line_blank: true,
            tab_indents: 0,
            space_indents: 0,
            space_widths: [0; 9],
            at_line_start: true,
            counting_indent: false,
            cur_indent_spaces: 0,
            in_word: false,
            prev_was_cr: false,
            last_char: None,
        }
    }

    fn consume(&mut self, ch: char) {
        self.chars += 1;

        if ch == '\n' {
            if self.prev_was_cr {
                self.crlf_count += 1;
                // Already finalised on the \r; nothing more to do.
            } else {
                self.lf_count += 1;
                self.finalize_line();
            }
            self.reset_line();
        } else if ch == '\r' {
            // Defer counting: the next char might be \n (→ CRLF) or not (→ CR).
            self.finalize_line();
            self.reset_line();
        } else {
            if self.prev_was_cr {
                self.cr_count += 1;
            }
            self.cur_line_chars += 1;
            if !ch.is_whitespace() {
                self.cur_line_blank = false;
            }
            self.classify_indent(ch);
        }

        let is_ws = ch.is_whitespace();
        if !is_ws && !self.in_word {
            self.words += 1;
            self.in_word = true;
        } else if is_ws {
            self.in_word = false;
        }

        self.prev_was_cr = ch == '\r';
        self.last_char = Some(ch);
    }

    fn classify_indent(&mut self, ch: char) {
        if self.at_line_start {
            self.counting_indent = true;
            self.at_line_start = false;
        }
        if !self.counting_indent {
            return;
        }
        if ch == '\t' {
            self.tab_indents += 1;
            self.counting_indent = false;
        } else if ch == ' ' {
            self.cur_indent_spaces = self.cur_indent_spaces.saturating_add(1);
        } else {
            if self.cur_indent_spaces > 0 {
                self.space_indents += 1;
                let idx = self.cur_indent_spaces.min(8) as usize;
                self.space_widths[idx] += 1;
            }
            self.counting_indent = false;
        }
    }

    fn finalize_line(&mut self) {
        if self.cur_line_chars > self.longest {
            self.longest = self.cur_line_chars;
        }
        if self.cur_line_blank {
            self.blank_lines += 1;
        }
    }

    fn reset_line(&mut self) {
        self.cur_line_chars = 0;
        self.cur_line_blank = true;
        self.at_line_start = true;
        self.counting_indent = false;
        self.cur_indent_spaces = 0;
    }

    fn finish(mut self, encoding: Encoding, shebang: Option<String>) -> TextStats {
        // Trailing CR (no following LF) — count it.
        if matches!(self.last_char, Some('\r')) {
            self.cr_count += 1;
            self.finalize_line();
            self.reset_line();
        }
        // Final unterminated line counts as one in `str::lines()` semantics.
        let last_was_terminator = matches!(self.last_char, Some('\n') | Some('\r') | None);
        if !last_was_terminator {
            self.finalize_line();
        }

        let line_count = match self.last_char {
            None => 0,
            Some('\n') | Some('\r') => self.lf_count + self.crlf_count + self.cr_count,
            Some(_) => self.lf_count + self.crlf_count + self.cr_count + 1,
        };

        TextStats {
            line_count,
            word_count: self.words,
            char_count: self.chars,
            blank_lines: self.blank_lines,
            longest_line_chars: self.longest,
            line_endings: classify_line_endings(self.lf_count, self.crlf_count, self.cr_count),
            indent_style: classify_indent(self.tab_indents, self.space_indents, &self.space_widths),
            encoding,
            shebang,
        }
    }
}

fn classify_line_endings(lf: usize, crlf: usize, cr: usize) -> LineEndings {
    let kinds = [
        (lf, LineEndings::Lf),
        (crlf, LineEndings::Crlf),
        (cr, LineEndings::Cr),
    ];
    let nonzero: Vec<_> = kinds.iter().filter(|(n, _)| *n > 0).collect();
    match nonzero.len() {
        0 => LineEndings::None,
        1 => nonzero[0].1,
        _ => LineEndings::Mixed,
    }
}

fn classify_indent(tabs: usize, spaces: usize, widths: &[usize; 9]) -> Option<IndentStyle> {
    if tabs == 0 && spaces == 0 {
        return None;
    }
    if tabs > 0 && spaces > 0 {
        return Some(IndentStyle::Mixed);
    }
    if tabs > 0 {
        return Some(IndentStyle::Tabs);
    }
    // Pick most common space width (1..=8). 4 is the default tiebreaker.
    let (mut best_w, mut best_n) = (4u8, 0usize);
    for (w, &n) in widths.iter().enumerate().skip(1) {
        if n > best_n {
            best_n = n;
            best_w = w as u8;
        }
    }
    Some(IndentStyle::Spaces(best_w))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn stdin_source(text: &str) -> InputSource {
        InputSource::Stdin {
            data: Arc::from(text.as_bytes().to_vec().into_boxed_slice()),
        }
    }

    fn text_stats(s: &str) -> TextStats {
        gather_text_stats(&stdin_source(s)).expect("expected text stats")
    }

    fn extras_text(s: &str) -> (usize, usize, usize) {
        let st = text_stats(s);
        (st.line_count, st.word_count, st.char_count)
    }

    fn assert_matches_std(s: &str) {
        let (lines, words, chars) = extras_text(s);
        assert_eq!(lines, s.lines().count(), "lines for {s:?}");
        assert_eq!(words, s.split_whitespace().count(), "words for {s:?}");
        assert_eq!(chars, s.chars().count(), "chars for {s:?}");
    }

    #[test]
    fn empty_input() {
        let stats = text_stats("");
        assert_eq!(stats.line_count, 0);
        assert_eq!(stats.word_count, 0);
        assert_eq!(stats.char_count, 0);
        assert!(matches!(stats.line_endings, LineEndings::None));
    }

    #[test]
    fn unterminated_final_line() {
        assert_matches_std("alpha\nbeta\ngamma");
    }

    #[test]
    fn terminated_final_line() {
        assert_matches_std("alpha\nbeta\ngamma\n");
    }

    #[test]
    fn blank_lines() {
        assert_matches_std("\n\n\n");
        assert_matches_std("a\n\nb\n");
    }

    #[test]
    fn crlf_lines() {
        assert_matches_std("a\r\nb\r\nc\r\n");
        let stats = text_stats("a\r\nb\r\nc\r\n");
        assert!(matches!(stats.line_endings, LineEndings::Crlf));
    }

    #[test]
    fn lf_classification() {
        let stats = text_stats("a\nb\n");
        assert!(matches!(stats.line_endings, LineEndings::Lf));
    }

    #[test]
    fn mixed_line_endings() {
        let stats = text_stats("a\nb\r\nc\n");
        assert!(matches!(stats.line_endings, LineEndings::Mixed));
    }

    #[test]
    fn longest_line_tracked() {
        let stats = text_stats("ab\nabcdef\nabc\n");
        assert_eq!(stats.longest_line_chars, 6);
    }

    #[test]
    fn blank_line_count() {
        let stats = text_stats("a\n\n\nb\n");
        assert_eq!(stats.blank_lines, 2);
    }

    #[test]
    fn indent_tabs() {
        let stats = text_stats("a\n\tb\n\tc\n");
        assert!(matches!(stats.indent_style, Some(IndentStyle::Tabs)));
    }

    #[test]
    fn indent_spaces() {
        let stats = text_stats("a\n    b\n    c\n");
        assert!(matches!(stats.indent_style, Some(IndentStyle::Spaces(4))));
    }

    #[test]
    fn unicode_words_and_chars() {
        assert_matches_std("héllo wörld\nαβγ δεζ\n你好 世界\n");
    }

    #[test]
    fn shebang_detected() {
        let stats = text_stats("#!/usr/bin/env python3\nprint('hi')\n");
        assert_eq!(stats.shebang.as_deref(), Some("/usr/bin/env python3"));
    }

    #[test]
    fn utf8_bom_detected() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"hello\n");
        let src = InputSource::Stdin {
            data: Arc::from(bytes.into_boxed_slice()),
        };
        let stats = gather_text_stats(&src).unwrap();
        assert!(matches!(stats.encoding, Encoding::Utf8Bom));
    }

    #[test]
    fn invalid_utf8_returns_none() {
        // 0x80 alone is invalid UTF-8 (continuation byte without lead).
        let bad = vec![0x80, 0x80, 0x80];
        let src = InputSource::Stdin {
            data: Arc::from(bad.into_boxed_slice()),
        };
        assert!(gather_text_stats(&src).is_none());
    }

    #[test]
    fn truncated_utf8_returns_none() {
        let truncated = vec![0xe4, 0xbd];
        let src = InputSource::Stdin {
            data: Arc::from(truncated.into_boxed_slice()),
        };
        assert!(gather_text_stats(&src).is_none());
    }
}
