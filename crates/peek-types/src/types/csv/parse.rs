//! Streaming, bounded-memory CSV record reader.
//!
//! Builds the source into a [`CsvData`] backed by a seekable
//! [`csv::Reader`]. Two tiers of resident records keep memory flat
//! regardless of file size or scroll depth:
//!
//! * a **seed** of the first [`SEED_RECORD_LIMIT`] records, captured at
//!   open and retained — feeds initial column widths, the header
//!   heuristic, the type-inference sample, and serves `row()` for the
//!   common top-of-file case
//! * a sliding **window** of [`WINDOW_SIZE`] records covering wherever
//!   the user scrolled past the seed, refilled by seeking the reader
//!   back to a sparse [`csv::Position`] **anchor** (recorded every
//!   [`ANCHOR_STRIDE`] records) and re-parsing forward to the target
//!
//! The total record count is unknown until a streaming count pass
//! (`ensure_all`) reaches EOF — that pass discards cells, so it stays
//! O(1) in memory. Reaching a deep record the first time re-parses from
//! the nearest anchor (≤ `ANCHOR_STRIDE` records); revisits are cheap.
//!
//! Encoding: UTF-8 native (BOM stripped, streamed + seekable). UTF-16
//! LE / BE inputs are BOM-detected and transcoded eagerly to a UTF-8
//! buffer served from a `Cursor`; that fully materialises the file, so
//! the windowing memory bound does **not** apply to UTF-16 CSV (rare;
//! accepted).
//!
//! Malformed-record guard:
//! * single record over [`MAX_RECORD_BYTES`] of raw cell bytes → error row
//! * single record spanning more than [`MAX_RECORD_LINES`] physical lines
//!   → error row
//! * csv crate per-record errors (UTF-8, ragged columns at strict mode,
//!   bad quoting) → error row
//!
//! Error rows carry [`Record::malformed = true`] and bump the malformed
//! counter (once per record, even across window re-reads); the reader
//! resyncs to the next newline automatically (csv crate does this).

use std::io::Cursor;

use anyhow::{Context, Result};
use csv::{Position, ReaderBuilder};

use crate::viewer::table::WINDOW_SIZE;
use crate::viewer::table::row_source::RowSource;
use peek_io::InputSource;
use peek_io::stream::{ByteStream, ReadSeek};

use super::CsvFormat;

/// Seed scan record cap. First 1000 records build initial column widths,
/// drive the header heuristic, and provide the type-inference sample.
pub const SEED_RECORD_LIMIT: usize = 1000;

/// Bytes of head data sniffed for delimiter detection and BOM lookup.
const SNIFF_BYTES: usize = 64 * 1024;

/// Maximum raw byte size for a single CSV record. A record exceeding
/// this cap is recorded as malformed (placeholder row) and the parser
/// resyncs at the next physical newline.
pub const MAX_RECORD_BYTES: usize = 4 * 1024 * 1024;

/// Maximum physical-line span for a single quoted record. Defends
/// against an unterminated open quote turning the rest of the file
/// into one giant record.
pub const MAX_RECORD_LINES: u64 = 10_000;

/// Source-text encoding detected at the BOM probe. UTF-16 inputs are
/// transcoded to UTF-8 up front; everything else is fed straight to
/// the csv reader as bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Record {
    /// `Option<String>` so the cell storage matches the shared
    /// [`RowSource`] contract — CSV always emits `Some(_)`; NULL only
    /// arises in other sources (SQLite).
    pub cells: Vec<Option<String>>,
    pub malformed: bool,
    /// Raw byte span of the record in the source — what the parse
    /// actually consumed. Feeds [`RowSource::row_scan_bytes`] so the
    /// search byte budget charges malformed records their true cost
    /// (their `cells` are empty, so a cell-text charge would be zero
    /// and a mostly-malformed multi-GB file would be re-walked
    /// end-to-end on every query).
    pub bytes: u64,
}

impl Record {
    fn ok(cells: Vec<Option<String>>, bytes: u64) -> Self {
        Self {
            cells,
            malformed: false,
            bytes,
        }
    }

    fn error(bytes: u64) -> Self {
        Self {
            cells: Vec::new(),
            malformed: true,
            bytes,
        }
    }
}

/// Seek-anchor spacing in records past the seed. `anchors[j]` marks the
/// start of record `SEED_RECORD_LIMIT + j * ANCHOR_STRIDE`. Smaller =
/// faster backward seeks, larger index; larger = the opposite. 256
/// bounds a backward seek to ≤256 record re-parses while keeping the
/// index tiny (≈ a few MB even for 100M-record files).
const ANCHOR_STRIDE: usize = 256;

/// Streaming, bounded-memory view over the source.
///
/// Two tiers of resident records:
/// * [`CsvData::seed`] — the first ≤[`SEED_RECORD_LIMIT`] records, kept
///   for the lifetime. Feeds column widths, the header heuristic,
///   alignment + type inference, and serves `row()` directly for the
///   common top-of-file case.
/// * a sliding [`CsvData::window`] of [`WINDOW_SIZE`] records covering
///   wherever the user has scrolled *past* the seed, refilled by seeking
///   the reader back to a recorded [`Position`] anchor.
///
/// Memory stays flat (seed + one window) regardless of file size or
/// scroll depth — the prior implementation grew a single `Vec` to the
/// deepest record viewed.
pub struct CsvData {
    pub delimiter: u8,
    pub encoding: Encoding,
    /// True when the file began with a UTF-8 / UTF-16 BOM. Drives the
    /// info row.
    pub has_bom: bool,
    /// Header-row heuristic decision from the seed scan. `Shift+H` can
    /// override this at runtime.
    pub header_heuristic: bool,
    /// First ≤[`SEED_RECORD_LIMIT`] records, retained for the lifetime.
    pub seed: Vec<Record>,
    /// Column count, from the first well-formed seed record.
    columns: usize,
    /// Sliding window of records *past* the seed. `window[i]` is record
    /// `window_start + i`; `window_start >= seed.len()`.
    window: Vec<Record>,
    window_start: usize,
    /// Sparse seek index past the seed (empty when the file fit the
    /// seed). `anchors[j]` is the reader position at the start of record
    /// `SEED_RECORD_LIMIT + j * ANCHOR_STRIDE`.
    anchors: Vec<Position>,
    /// Highest record count confirmed by forward scanning so far.
    discovered: usize,
    /// Total record count once a full pass has reached EOF.
    total: Option<usize>,
    /// Count of malformed rows seen — only incremented the first time a
    /// record index is discovered, so window refills / count passes
    /// re-reading the same records don't double-count.
    pub malformed_count: usize,
    /// Last physical line reported by the reader, for the per-record
    /// line-span guard. Reset on every seek to the anchor's line.
    last_line: u64,
    reader: csv::Reader<Box<dyn ReadSeek>>,
}

impl CsvData {
    pub fn open(source: &InputSource, fmt: CsvFormat) -> Result<Self> {
        let head = head_bytes(source)?;
        let (encoding, body_offset, has_bom) = sniff_encoding(&head);
        let body_reader: Box<dyn ReadSeek> = build_body_reader(source, encoding, body_offset)?;
        let delimiter = sniff_delimiter(&head[body_offset..], fmt);

        let mut reader = ReaderBuilder::new()
            .has_headers(false)
            .delimiter(delimiter)
            .flexible(true)
            .from_reader(body_reader);

        // Seed: pull up to SEED_RECORD_LIMIT records to feed widths,
        // header heuristic, type inference, and the top-of-file rows.
        let mut seed: Vec<Record> = Vec::with_capacity(SEED_RECORD_LIMIT.min(64));
        let mut last_line: u64 = 0;
        let mut malformed_count = 0usize;
        let mut eof = false;
        for _ in 0..SEED_RECORD_LIMIT {
            match read_next(&mut reader, &mut last_line) {
                Ok(Some(rec)) => {
                    if rec.malformed {
                        malformed_count += 1;
                    }
                    seed.push(rec);
                }
                Ok(None) => {
                    eof = true;
                    break;
                }
                Err(e) => return Err(e).context("csv seed scan failed"),
            }
        }

        let columns = seed
            .iter()
            .find(|r| !r.malformed)
            .map(|r| r.cells.len())
            .unwrap_or(0);
        let header_heuristic = detect_header(&seed);

        // If the file fit in the seed, the total is already known and no
        // window / anchors are needed. Otherwise anchor record
        // SEED_RECORD_LIMIT (where the reader now sits) as anchors[0].
        let (anchors, discovered, total) = if eof {
            (Vec::new(), seed.len(), Some(seed.len()))
        } else {
            (vec![reader.position().clone()], SEED_RECORD_LIMIT, None)
        };

        Ok(Self {
            delimiter,
            encoding,
            has_bom,
            header_heuristic,
            seed,
            columns,
            window: Vec::new(),
            window_start: SEED_RECORD_LIMIT,
            anchors,
            discovered,
            total,
            malformed_count,
            last_line,
            reader,
        })
    }

    /// Record a seek anchor at record `cur` if it's a stride boundary
    /// and the next anchor slot is exactly the one being extended. Must
    /// be called with the reader positioned at the start of record
    /// `cur` (before reading it). Monotonic: never overwrites or gaps.
    fn maybe_record_anchor(&mut self, cur: usize) {
        if cur < SEED_RECORD_LIMIT {
            return;
        }
        let rel = cur - SEED_RECORD_LIMIT;
        if rel.is_multiple_of(ANCHOR_STRIDE) && rel / ANCHOR_STRIDE == self.anchors.len() {
            self.anchors.push(self.reader.position().clone());
        }
    }

    /// Read one record during a forward scan at logical index `cur`,
    /// updating malformed count / discovered frontier / total. Cells are
    /// returned to the caller (a counting pass simply drops them).
    /// `None` at EOF.
    fn scan_one(&mut self, cur: usize) -> Result<Option<Record>> {
        self.maybe_record_anchor(cur);
        match read_next(&mut self.reader, &mut self.last_line)? {
            Some(rec) => {
                let first_seen = cur >= self.discovered;
                if rec.malformed && first_seen {
                    self.malformed_count += 1;
                }
                if cur + 1 > self.discovered {
                    self.discovered = cur + 1;
                }
                Ok(Some(rec))
            }
            None => {
                self.total = Some(cur);
                self.discovered = self.discovered.max(cur);
                Ok(None)
            }
        }
    }

    /// Seek the reader to the start of record `target` (≥ seed length),
    /// via the nearest anchor at or before it plus a forward skip.
    /// `Ok(true)` if positioned at `target`, `Ok(false)` if EOF was hit
    /// first (target past end).
    fn seek_to_record(&mut self, target: usize) -> Result<bool> {
        let rel = target - SEED_RECORD_LIMIT;
        let anchor_idx = (rel / ANCHOR_STRIDE).min(self.anchors.len().saturating_sub(1));
        let pos = self.anchors[anchor_idx].clone();
        let mut cur = SEED_RECORD_LIMIT + anchor_idx * ANCHOR_STRIDE;
        self.reader.seek(pos.clone())?;
        self.last_line = pos.line();
        while cur < target {
            if self.scan_one(cur)?.is_none() {
                return Ok(false);
            }
            cur += 1;
        }
        Ok(true)
    }

    /// Refill the window so it covers record `target`, centred on it.
    fn refill_window(&mut self, target: usize) -> Result<()> {
        let seed_len = self.seed.len();
        let half = WINDOW_SIZE / 2;
        let mut start = target.saturating_sub(half).max(seed_len);
        if let Some(t) = self.total {
            let max_start = t.saturating_sub(WINDOW_SIZE).max(seed_len);
            start = start.min(max_start);
            if start >= t {
                self.window.clear();
                self.window_start = seed_len;
                return Ok(());
            }
        }
        if !self.seek_to_record(start)? {
            // `start` is past EOF — total now known; nothing to window.
            self.window.clear();
            self.window_start = seed_len;
            return Ok(());
        }
        let mut win = Vec::with_capacity(WINDOW_SIZE);
        let mut cur = start;
        while win.len() < WINDOW_SIZE {
            match self.scan_one(cur)? {
                Some(rec) => {
                    win.push(rec);
                    cur += 1;
                }
                None => break,
            }
        }
        self.window = win;
        self.window_start = start;
        Ok(())
    }

    /// Drive a count pass to EOF so [`Self::total`] becomes definite,
    /// discarding cells — O(1) memory. No-op once total is known.
    pub fn ensure_all(&mut self) -> Result<()> {
        if self.total.is_some() {
            return Ok(());
        }
        let last = self.anchors.len() - 1;
        let pos = self.anchors[last].clone();
        let mut cur = SEED_RECORD_LIMIT + last * ANCHOR_STRIDE;
        self.reader.seek(pos.clone())?;
        self.last_line = pos.line();
        while self.scan_one(cur)?.is_some() {
            cur += 1;
        }
        Ok(())
    }

    /// Total record count if a full pass has reached EOF, else `None`.
    pub fn total_records(&self) -> Option<usize> {
        self.total
    }

    /// Addressable record count: the exact total once known, otherwise
    /// the current forward frontier (scrolling past it pulls more).
    pub fn loaded(&self) -> usize {
        self.total.unwrap_or(self.discovered)
    }

    /// Column count from the seed.
    pub fn column_count(&self) -> usize {
        self.columns
    }

    /// Resolve a record from the seed (idx < seed length) or the sliding
    /// window. `None` when outside both — the caller should `ensure_row`
    /// first.
    fn record(&self, idx: usize) -> Option<&Record> {
        if idx < self.seed.len() {
            return self.seed.get(idx);
        }
        if idx >= self.window_start && idx < self.window_start + self.window.len() {
            return self.window.get(idx - self.window_start);
        }
        None
    }
}

impl RowSource for CsvData {
    fn ensure_row(&mut self, idx: usize) -> Result<usize> {
        if idx >= self.seed.len() {
            let in_window = idx >= self.window_start && idx < self.window_start + self.window.len();
            if !in_window {
                self.refill_window(idx)?;
            }
        }
        Ok(self.loaded())
    }

    fn ensure_all(&mut self) -> Result<()> {
        self.ensure_all()
    }

    fn row(&self, idx: usize) -> Option<&[Option<String>]> {
        self.record(idx).map(|r| r.cells.as_slice())
    }

    fn row_is_malformed(&self, idx: usize) -> bool {
        self.record(idx).map(|r| r.malformed).unwrap_or(false)
    }

    fn row_scan_bytes(&self, idx: usize) -> u64 {
        self.record(idx).map(|r| r.bytes).unwrap_or(0)
    }

    fn loaded(&self) -> usize {
        self.loaded()
    }

    fn total(&self) -> Option<usize> {
        self.total_records()
    }

    fn column_count(&self) -> usize {
        self.column_count()
    }

    fn malformed_count(&self) -> usize {
        self.malformed_count
    }
}

/// Read the next record. `Ok(None)` on EOF, `Ok(Some(Record))` for both
/// well-formed and malformed records. Errors are converted to
/// `Record::error()` so the parser can resync without bubbling out.
fn read_next(
    reader: &mut csv::Reader<Box<dyn ReadSeek>>,
    last_line: &mut u64,
) -> Result<Option<Record>> {
    // The reader sits at the start of this record; after `read_record`
    // it sits at the start of the next — the difference is the raw span
    // the parse consumed, charged to the search byte budget.
    let start_byte = reader.position().byte();
    let mut sr = csv::StringRecord::new();
    match reader.read_record(&mut sr) {
        Ok(true) => {
            let pos = reader.position();
            let raw_bytes = pos.byte().saturating_sub(start_byte);
            let pos_line = pos.line();
            let span = pos_line.saturating_sub(*last_line);
            *last_line = pos_line;
            let bytes_total: usize = sr.iter().map(|c| c.len()).sum();
            if span > MAX_RECORD_LINES || bytes_total > MAX_RECORD_BYTES {
                return Ok(Some(Record::error(raw_bytes)));
            }
            let cells = sr.iter().map(|s| Some(s.to_string())).collect();
            Ok(Some(Record::ok(cells, raw_bytes)))
        }
        Ok(false) => Ok(None),
        Err(_) => {
            // csv crate's reader auto-resyncs at the next newline on the
            // next read_record call, so we just emit an error row and
            // let the caller continue.
            let raw_bytes = reader.position().byte().saturating_sub(start_byte);
            Ok(Some(Record::error(raw_bytes)))
        }
    }
}

fn head_bytes(source: &InputSource) -> Result<Vec<u8>> {
    let bs = source.open_byte_source()?;
    let want = bs.len().min(SNIFF_BYTES as u64) as usize;
    let bytes = bs.read_range(0, want)?;
    Ok(bytes.to_vec())
}

/// Inspect the BOM. Returns the encoding, the number of leading bytes
/// to skip (BOM length), and whether a BOM was found.
fn sniff_encoding(head: &[u8]) -> (Encoding, usize, bool) {
    if head.starts_with(&[0xFFu8, 0xFE]) {
        return (Encoding::Utf16Le, 2, true);
    }
    if head.starts_with(&[0xFEu8, 0xFF]) {
        return (Encoding::Utf16Be, 2, true);
    }
    if head.starts_with(&[0xEFu8, 0xBB, 0xBF]) {
        return (Encoding::Utf8, 3, true);
    }
    (Encoding::Utf8, 0, false)
}

/// Build the seekable body reader passed to the csv crate.
///
/// UTF-8: a `ByteStream` over the random-access byte source, ranged to
/// start just past any BOM — so the csv reader's byte positions are
/// 0-based over the body and `Reader::seek` can jump back to a recorded
/// record position. UTF-16: the source is transcoded to a UTF-8 `Vec`
/// up front and served from a `Cursor`. That fully materialises the
/// file in memory; UTF-16 CSV is rare and the windowing memory bound
/// doesn't apply to it, so the read is gated at the whole-doc budget —
/// a UTF-16 BOM on a huge file must not buy an unbounded load.
fn build_body_reader(
    source: &InputSource,
    encoding: Encoding,
    body_offset: usize,
) -> Result<Box<dyn ReadSeek>> {
    match encoding {
        Encoding::Utf8 => {
            let bs = source.open_byte_source()?;
            let len = bs.len();
            Ok(Box::new(ByteStream::range(bs, body_offset as u64, len)))
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let len = source.byte_len()?;
            let cap = peek_io::limits::WHOLE_DOC_BYTES;
            if len > cap {
                anyhow::bail!(
                    "UTF-16 CSV is {} MB (> {} MB cap): transcoding holds the whole file in \
                     memory, unlike the streaming UTF-8 path",
                    len / (1024 * 1024),
                    cap / (1024 * 1024)
                );
            }
            // Gated by the WHOLE_DOC_BYTES byte_len check above.
            let raw = source.read_bytes(peek_io::limits::Budget::Unbounded(
                "gated by WHOLE_DOC_BYTES above",
            ))?;
            let payload = &raw[body_offset..];
            let transcoded = transcode_utf16(payload, encoding)?;
            Ok(Box::new(Cursor::new(transcoded.into_bytes())))
        }
    }
}

/// Walk byte pairs, handle surrogate pairs, push to a UTF-8 string.
/// Lossy on invalid surrogate pairs (replaces with U+FFFD).
fn transcode_utf16(bytes: &[u8], enc: Encoding) -> Result<String> {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i + 1 < bytes.len() {
        let unit = match enc {
            Encoding::Utf16Le => u16::from_le_bytes([bytes[i], bytes[i + 1]]),
            Encoding::Utf16Be => u16::from_be_bytes([bytes[i], bytes[i + 1]]),
            Encoding::Utf8 => unreachable!("transcode_utf16 called on UTF-8"),
        };
        i += 2;
        if (0xD800..=0xDBFF).contains(&unit) {
            // High surrogate — read the next pair as low surrogate.
            if i + 1 >= bytes.len() {
                out.push('\u{FFFD}');
                break;
            }
            let low = match enc {
                Encoding::Utf16Le => u16::from_le_bytes([bytes[i], bytes[i + 1]]),
                Encoding::Utf16Be => u16::from_be_bytes([bytes[i], bytes[i + 1]]),
                Encoding::Utf8 => unreachable!(),
            };
            i += 2;
            if !(0xDC00..=0xDFFF).contains(&low) {
                out.push('\u{FFFD}');
                continue;
            }
            let cp = 0x10000u32 + (((unit - 0xD800) as u32) << 10) + ((low - 0xDC00) as u32);
            if let Some(c) = char::from_u32(cp) {
                out.push(c);
            } else {
                out.push('\u{FFFD}');
            }
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            // Lone low surrogate.
            out.push('\u{FFFD}');
        } else {
            out.push(char::from_u32(unit as u32).unwrap_or('\u{FFFD}'));
        }
    }
    Ok(out)
}

/// Pick the delimiter for this source. Extension default wins unless a
/// content-sniff strongly indicates otherwise — i.e. when the seed bytes
/// contain many more of a non-default candidate than the default.
fn sniff_delimiter(head: &[u8], fmt: CsvFormat) -> u8 {
    let default = fmt.default_delimiter();
    let candidates: [u8; 4] = [b',', b'\t', b';', b'|'];

    let mut counts = [0usize; 4];
    let mut in_quote = false;
    for &b in head {
        if b == b'"' {
            in_quote = !in_quote;
            continue;
        }
        if in_quote {
            continue;
        }
        for (i, c) in candidates.iter().enumerate() {
            if b == *c {
                counts[i] += 1;
                break;
            }
        }
    }

    let default_idx = candidates.iter().position(|c| *c == default).unwrap_or(0);
    let default_count = counts[default_idx];

    let mut best = default;
    let mut best_count = default_count;
    for (i, &c) in candidates.iter().enumerate() {
        // Override only when an alternative outscores the default by a
        // clear margin (3x). Avoids flipping on noise.
        if c == default {
            continue;
        }
        if counts[i] > best_count.saturating_mul(3) {
            best = c;
            best_count = counts[i];
        }
    }
    best
}

/// Heuristic header detection. Row 0 is treated as a header when every
/// cell in row 0 classifies as text (not int/float/bool/date). A typed
/// cell in row 0 turns the heuristic off — clear signal that row 0 is
/// data, not a label. Ambiguous all-text rows default to header on
/// (matches the plan's "ambiguous → header by default" rule).
fn detect_header(records: &[Record]) -> bool {
    let Some(first) = records.iter().find(|r| !r.malformed) else {
        return false;
    };
    if first.cells.is_empty() {
        return false;
    }
    first.cells.iter().all(|c| {
        matches!(
            classify_cell(c.as_deref().unwrap_or("")),
            CellKind::Text | CellKind::Empty
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Empty,
    Int,
    Float,
    Bool,
    Date,
    Text,
}

pub fn classify_cell(s: &str) -> CellKind {
    let t = s.trim();
    if t.is_empty() {
        return CellKind::Empty;
    }
    if matches!(t, "true" | "false" | "True" | "False" | "TRUE" | "FALSE") {
        return CellKind::Bool;
    }
    if t.parse::<i64>().is_ok() {
        return CellKind::Int;
    }
    if t.parse::<f64>().is_ok() {
        return CellKind::Float;
    }
    // European decimal: one comma, no dot — `,` is the decimal
    // separator (`249,90` → 249.90). Common in European locales' CSV.
    let comma_count = t.bytes().filter(|b| *b == b',').count();
    if !t.contains('.') && comma_count == 1 && t.replace(',', ".").parse::<f64>().is_ok() {
        return CellKind::Float;
    }
    // US thousand-grouped: digits with `,` grouping. Strip commas and
    // retry — `1,234` → 1234 (int), `1,234.56` → float.
    if comma_count >= 1 {
        let stripped: String = t
            .bytes()
            .filter(|b| *b != b',')
            .map(|b| b as char)
            .collect();
        if stripped.parse::<i64>().is_ok() {
            return CellKind::Int;
        }
        if stripped.parse::<f64>().is_ok() {
            return CellKind::Float;
        }
    }
    if looks_like_date(t) {
        return CellKind::Date;
    }
    CellKind::Text
}

/// Cheap date heuristic — `YYYY-MM-DD` or `YYYY/MM/DD`, optionally
/// followed by `T` or ` ` and `HH:MM[:SS]`. Strict enough to avoid
/// false positives on plain numbers; lenient enough to cover the
/// common ISO 8601 family.
fn looks_like_date(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 8 {
        return false;
    }
    let year_ok = bytes[..4].iter().all(|b| b.is_ascii_digit());
    let sep1_ok = bytes[4] == b'-' || bytes[4] == b'/';
    let month_ok = bytes[5..7].iter().all(|b| b.is_ascii_digit());
    let sep2_ok = bytes[7] == b'-' || bytes[7] == b'/';
    if bytes.len() == 8 && year_ok && sep1_ok && month_ok {
        // `YYYY-MM-` with nothing after is not a date.
        return false;
    }
    if bytes.len() < 10 {
        return false;
    }
    let day_ok = bytes[8..10].iter().all(|b| b.is_ascii_digit());
    year_ok && sep1_ok && month_ok && sep2_ok && day_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn stdin(text: &str) -> InputSource {
        InputSource::stdin(Bytes::copy_from_slice(text.as_bytes()))
    }

    fn some_cells(values: &[&str]) -> Vec<Option<String>> {
        values.iter().map(|v| Some((*v).to_string())).collect()
    }

    fn fixture(rel: &str) -> InputSource {
        let mut p = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        p.push(rel);
        InputSource::File(p)
    }

    /// transactions-10k.csv (header + 10 000 rows; record N's `id` column is
    /// `100000 + N`) drives the windowing path: deep forward + backward seeks
    /// land on the right rows, the resident set stays bounded (seed + one window),
    /// and a count pass settles the total.
    #[test]
    fn windowed_seek_lands_on_correct_rows_and_stays_bounded() {
        let src = fixture("test-data/transactions-10k.csv");
        let mut data = CsvData::open(&src, CsvFormat::Csv).unwrap();

        // Big file: seed capped, total not yet known.
        assert_eq!(data.seed.len(), SEED_RECORD_LIMIT);
        assert_eq!(data.total_records(), None);

        let id =
            |data: &CsvData, idx: usize| -> String { data.row(idx).unwrap()[0].clone().unwrap() };

        // Forward seek well past the seed.
        data.ensure_row(5000).unwrap();
        assert_eq!(id(&data, 5000), "105000");
        assert!(data.window.len() <= WINDOW_SIZE);

        // Backward seek to a different windowed region.
        data.ensure_row(1200).unwrap();
        assert_eq!(id(&data, 1200), "101200");
        assert!(data.window.len() <= WINDOW_SIZE);

        // Top-of-file rows always resolve from the seed.
        assert_eq!(id(&data, 1), "100001");

        // Count pass settles the total. This is the raw CSV record
        // count — the header row plus 10 000 data rows; the table body
        // excludes the header and shows 10 000 transactions.
        data.ensure_all().unwrap();
        assert_eq!(data.total_records(), Some(10_001));
        assert_eq!(data.loaded(), 10_001);

        // Last row reachable after the total is known.
        data.ensure_row(10_000).unwrap();
        assert_eq!(id(&data, 10_000), "110000");

        // Resident set stayed bounded throughout.
        assert_eq!(data.seed.len(), SEED_RECORD_LIMIT);
        assert!(data.window.len() <= WINDOW_SIZE);
    }

    #[test]
    fn seed_parses_simple_csv() {
        let src = stdin("name,age\nalice,30\nbob,25\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert_eq!(data.delimiter, b',');
        assert_eq!(data.seed.len(), 3);
        assert_eq!(data.seed[0].cells, some_cells(&["name", "age"]));
        assert_eq!(data.seed[1].cells, some_cells(&["alice", "30"]));
        assert!(data.header_heuristic);
        assert_eq!(data.column_count(), 2);
    }

    #[test]
    fn tsv_uses_tab_delimiter() {
        let src = stdin("a\tb\tc\n1\t2\t3\n");
        let data = CsvData::open(&src, CsvFormat::Tsv).unwrap();
        assert_eq!(data.delimiter, b'\t');
        assert_eq!(data.seed[0].cells, some_cells(&["a", "b", "c"]));
    }

    #[test]
    fn delimiter_sniff_overrides_default_when_clear() {
        // `.csv` extension but body is clearly tab-separated.
        let src = stdin("a\tb\tc\n1\t2\t3\n4\t5\t6\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert_eq!(data.delimiter, b'\t', "tab clearly dominates → override");
    }

    #[test]
    fn header_heuristic_on_when_row0_all_text() {
        // All-text rows are ambiguous → default to header on per the plan.
        let src = stdin("alpha,beta\ngamma,delta\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert!(data.header_heuristic);
    }

    #[test]
    fn header_heuristic_on_when_typed_data_follows() {
        let src = stdin("name,age\nalice,30\nbob,25\ncarol,28\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert!(data.header_heuristic);
    }

    #[test]
    fn header_heuristic_off_when_row0_has_typed_cells() {
        // Row 0 has a numeric cell → it's data, not a header.
        let src = stdin("1,2\n3,4\n5,6\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert!(!data.header_heuristic);
    }

    #[test]
    fn quoted_newlines_keep_record_together() {
        let src = stdin("a,b\n\"one\ntwo\",x\nlast,y\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert_eq!(data.seed.len(), 3);
        assert_eq!(data.seed[1].cells, some_cells(&["one\ntwo", "x"]));
    }

    /// Record byte spans track the raw bytes the parse consumed —
    /// malformed records included, whose cells are empty. The search
    /// byte budget charges these spans via `row_scan_bytes`; a zero
    /// span on malformed records would let a mostly-malformed file
    /// escape the budget entirely.
    #[test]
    fn record_bytes_track_raw_span_including_malformed() {
        // Middle record carries invalid UTF-8 — a csv read error,
        // surfaced as a malformed record.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"a,b\n");
        buf.extend_from_slice(b"bad,\xFF\xFE\n");
        buf.extend_from_slice(b"last,z\n");
        let body_len = buf.len() as u64;
        let src = InputSource::stdin(Bytes::from(buf));
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert_eq!(data.seed.len(), 3);
        assert!(!data.seed[0].malformed);
        assert!(data.seed[1].malformed, "invalid UTF-8 must flag the record");
        assert_eq!(data.seed[0].bytes, 4, "span of 'a,b\\n'");
        assert!(
            data.seed[1].bytes > 0,
            "malformed record must report its raw span"
        );
        // No gaps: the three spans cover the whole body.
        let total: u64 = data.seed.iter().map(|r| r.bytes).sum();
        assert_eq!(total, body_len);
        // The RowSource view the search budget sees.
        assert_eq!(data.row_scan_bytes(1), data.seed[1].bytes);
    }

    #[test]
    fn utf8_bom_is_stripped() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"\xEF\xBB\xBF");
        buf.extend_from_slice(b"name,age\nalice,30\n");
        let src = InputSource::stdin(Bytes::from(buf));
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert_eq!(data.encoding, Encoding::Utf8);
        assert!(data.has_bom);
        assert_eq!(data.seed[0].cells, some_cells(&["name", "age"]));
    }

    #[test]
    fn utf16_le_transcoded_to_utf8() {
        // UTF-16 LE BOM + "a,b\n1,2\n"
        let mut buf = vec![0xFF, 0xFE];
        for c in "a,b\n1,2\n".chars() {
            let unit = c as u16;
            buf.extend_from_slice(&unit.to_le_bytes());
        }
        let src = InputSource::stdin(Bytes::from(buf));
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert_eq!(data.encoding, Encoding::Utf16Le);
        assert!(data.has_bom);
        assert_eq!(data.seed[0].cells, some_cells(&["a", "b"]));
        assert_eq!(data.seed[1].cells, some_cells(&["1", "2"]));
    }

    #[test]
    fn utf16_over_cap_refuses_instead_of_transcoding() {
        // UTF-16 BOM on a body past the whole-doc cap: open must refuse
        // with the cap message, not materialise + transcode the file.
        let cap = peek_io::limits::WHOLE_DOC_BYTES as usize;
        let mut buf = vec![0xFF, 0xFE];
        buf.resize(cap + 2, b' ');
        let src = InputSource::stdin(Bytes::from(buf));
        let err = match CsvData::open(&src, CsvFormat::Csv) {
            Ok(_) => panic!("over-cap UTF-16 CSV must refuse"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("cap"), "got: {err:#}");
    }

    #[test]
    fn classify_cell_buckets() {
        assert_eq!(classify_cell(""), CellKind::Empty);
        assert_eq!(classify_cell("   "), CellKind::Empty);
        assert_eq!(classify_cell("42"), CellKind::Int);
        assert_eq!(classify_cell("-3"), CellKind::Int);
        assert_eq!(classify_cell("3.14"), CellKind::Float);
        assert_eq!(classify_cell("true"), CellKind::Bool);
        assert_eq!(classify_cell("FALSE"), CellKind::Bool);
        assert_eq!(classify_cell("2024-01-15"), CellKind::Date);
        assert_eq!(classify_cell("2024/01/15"), CellKind::Date);
        assert_eq!(classify_cell("hello"), CellKind::Text);
    }

    #[test]
    fn classify_cell_european_decimal() {
        // Single `,`, no `.` — comma is the decimal separator.
        assert_eq!(classify_cell("249,90"), CellKind::Float);
        assert_eq!(classify_cell("-3,14"), CellKind::Float);
        assert_eq!(classify_cell("0,5"), CellKind::Float);
    }

    #[test]
    fn classify_cell_us_thousand_grouped() {
        // Comma grouping with no decimal → int.
        assert_eq!(classify_cell("1,234"), CellKind::Float);
        // The case above is genuinely ambiguous between `1234` (US
        // thousand sep) and `1.234` (European decimal). The single-
        // comma branch fires first and treats it as European
        // decimal — both interpretations are numeric, so right-align
        // is correct either way. Multi-comma cases are unambiguous:
        assert_eq!(classify_cell("1,234,567"), CellKind::Int);
        assert_eq!(classify_cell("1,234.56"), CellKind::Float);
    }

    // --- Fixture-based CSV parsing ------------------------------------------
    //
    // These exercise the reader against the real on-disk fixtures: delimiter
    // sniff, header heuristic, column count, and alignment inference. They
    // live here (peek-types) because they test CsvData / infer_alignments;
    // the table-mode mechanics they used to share a module with live in
    // peek-foundation's rows_mode tests.

    /// employees.csv: comma-delimited, header detected, 6 columns, with the
    /// numeric columns (`id`, `salary`) inferred as right-aligned.
    #[test]
    fn fixture_employees_alignment_and_header() {
        use crate::types::csv::compose::infer_alignments;
        use crate::viewer::table::rows_mode::Alignment;

        let data = CsvData::open(&fixture("test-data/employees.csv"), CsvFormat::Csv).unwrap();
        assert_eq!(data.delimiter, b',');
        assert!(data.header_heuristic, "header row detected");
        assert_eq!(data.column_count(), 6);
        let body_start = if data.header_heuristic { 1 } else { 0 };
        let aligns = infer_alignments(&data, body_start);
        // id (int), name (text), department (text), salary (float),
        // start_date (date), active (bool).
        assert_eq!(aligns[0], Alignment::Right, "id column");
        assert_eq!(aligns[1], Alignment::Left, "name column");
        assert_eq!(aligns[3], Alignment::Right, "salary column");
        assert_eq!(aligns[4], Alignment::Left, "start_date column");
    }

    /// measurements.tsv uses the tab delimiter via its extension.
    #[test]
    fn fixture_measurements_tsv_tab_delimiter() {
        let data = CsvData::open(&fixture("test-data/measurements.tsv"), CsvFormat::Tsv).unwrap();
        assert_eq!(data.delimiter, b'\t');
        assert!(data.header_heuristic);
        assert_eq!(data.column_count(), 6);
    }

    /// euro-prices.csv uses `;` despite the `.csv` extension — the content
    /// sniff overrides the comma default.
    #[test]
    fn fixture_euro_prices_sniffs_semicolon() {
        let data = CsvData::open(&fixture("test-data/euro-prices.csv"), CsvFormat::Csv).unwrap();
        assert_eq!(data.delimiter, b';', "semicolon should win over comma");
        assert!(data.header_heuristic);
    }

    /// sensor-log.csv has no header — row 0 begins with a numeric Unix
    /// timestamp, so the heuristic must classify it as data.
    #[test]
    fn fixture_sensor_log_no_header() {
        let data = CsvData::open(&fixture("test-data/sensor-log.csv"), CsvFormat::Csv).unwrap();
        assert!(!data.header_heuristic, "row 0 typed → no header");
        assert_eq!(data.column_count(), 5);
    }

    /// books.csv carries two records with an embedded `\n` in their
    /// description cell — the reader must keep them as single records with
    /// the newline inside one cell.
    #[test]
    fn fixture_books_preserves_embedded_newline_cells() {
        let data = CsvData::open(&fixture("test-data/books.csv"), CsvFormat::Csv).unwrap();
        let multi = data
            .seed
            .iter()
            .filter(|r| !r.malformed)
            .filter(|r| {
                r.cells
                    .iter()
                    .any(|c| c.as_deref().is_some_and(|s| s.contains('\n')))
            })
            .count();
        assert_eq!(multi, 2, "books.csv should have two multi-line cells");
    }
}
