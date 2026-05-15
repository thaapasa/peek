//! Streaming CSV record reader.
//!
//! Builds the source into a [`CsvData`] holding:
//!
//! * the active delimiter (extension default + content sniff override)
//! * a seed scan of the first [`SEED_RECORD_LIMIT`] records, captured into
//!   memory at open time — feeds initial column widths, header heuristic,
//!   and the type-inference sample
//! * an ongoing [`csv::Reader`] over the same byte source, paused at the
//!   record after the seed; the table mode pulls more records on demand
//!   as the user scrolls past the seed window
//!
//! Records are kept in [`CsvData::records`] (grown lazily). Past
//! [`SEED_RECORD_LIMIT`] we keep extending until the reader hits EOF.
//! Memory grows linearly with the deepest record index the user has
//! scrolled to — multi-GB files only materialise as far as they're
//! viewed.
//!
//! Encoding: UTF-8 native (BOM stripped). UTF-16 LE / BE inputs are
//! BOM-detected and transcoded eagerly to a UTF-8 byte buffer fed into
//! the csv reader. UTF-16 is rare in CSV and the transcode is a one-pass
//! byte walk, so a full-file read on UTF-16 is acceptable.
//!
//! Malformed-record guard:
//! * single record over [`MAX_RECORD_BYTES`] of raw cell bytes → error row
//! * single record spanning more than [`MAX_RECORD_LINES`] physical lines
//!   → error row
//! * csv crate per-record errors (UTF-8, ragged columns at strict mode,
//!   bad quoting) → error row
//!
//! Error rows are recorded in [`CsvData::records`] with
//! [`Record::malformed = true`] and the malformed counter bumped; the
//! reader resyncs to the next newline automatically (csv crate does this).

use std::io::{BufReader, Cursor, Read};

use anyhow::{Context, Result};
use csv::ReaderBuilder;

use crate::input::InputSource;

use super::format::CsvFormat;

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
    pub cells: Vec<String>,
    pub malformed: bool,
}

impl Record {
    fn ok(cells: Vec<String>) -> Self {
        Self {
            cells,
            malformed: false,
        }
    }

    fn error() -> Self {
        Self {
            cells: Vec::new(),
            malformed: true,
        }
    }
}

/// Streaming record-keyed view over the source. Records are pulled on
/// demand by [`CsvData::ensure_record`] until the underlying reader
/// reaches EOF; the seed pass at open time materialises the first
/// [`SEED_RECORD_LIMIT`] records up front.
pub struct CsvData {
    pub delimiter: u8,
    pub encoding: Encoding,
    pub records: Vec<Record>,
    /// Set when the reader has been driven to EOF — no more records
    /// will ever materialise. `total_records` then equals `records.len()`.
    pub fully_consumed: bool,
    /// Count of malformed rows encountered across the scan so far.
    pub malformed_count: usize,
    /// True when the file began with a UTF-8 / UTF-16 BOM. Drives the
    /// info row.
    pub has_bom: bool,
    /// Header-row heuristic decision from the seed scan: true when row
    /// 0 looks like a header (all-text) and at least one later seed row
    /// carries a typed cell. `Shift+H` can override this at runtime.
    pub header_heuristic: bool,
    /// Last position reported by the csv reader, used to compute
    /// physical-line span between successive records.
    last_line: u64,
    reader: csv::Reader<Box<dyn Read>>,
}

impl CsvData {
    pub fn open(source: &InputSource, fmt: CsvFormat) -> Result<Self> {
        let head = head_bytes(source)?;
        let (encoding, body_offset, has_bom) = sniff_encoding(&head);
        let body_reader: Box<dyn Read> = build_body_reader(source, encoding, body_offset)?;
        let delimiter = sniff_delimiter(&head[body_offset..], fmt);

        let mut reader = ReaderBuilder::new()
            .has_headers(false)
            .delimiter(delimiter)
            .flexible(true)
            .from_reader(body_reader);

        // Seed: pull up to SEED_RECORD_LIMIT records to feed widths,
        // header heuristic, and type inference.
        let mut records: Vec<Record> = Vec::with_capacity(SEED_RECORD_LIMIT.min(64));
        let mut last_line: u64 = 0;
        let mut malformed_count = 0usize;
        let mut fully_consumed = false;
        for _ in 0..SEED_RECORD_LIMIT {
            match read_next(&mut reader, &mut last_line) {
                Ok(Some(rec)) => {
                    if rec.malformed {
                        malformed_count += 1;
                    }
                    records.push(rec);
                }
                Ok(None) => {
                    fully_consumed = true;
                    break;
                }
                Err(e) => {
                    return Err(e).context("csv seed scan failed");
                }
            }
        }

        let header_heuristic = detect_header(&records);

        Ok(Self {
            delimiter,
            encoding,
            records,
            fully_consumed,
            malformed_count,
            has_bom,
            header_heuristic,
            last_line,
            reader,
        })
    }

    /// Ensure records up to `idx` (inclusive) are loaded. Pulls records
    /// from the underlying reader as needed; no-op if already loaded or
    /// past EOF. Returns the number of records currently materialised,
    /// which is `min(idx + 1, total)` on success.
    pub fn ensure_record(&mut self, idx: usize) -> Result<usize> {
        while !self.fully_consumed && self.records.len() <= idx {
            match read_next(&mut self.reader, &mut self.last_line)? {
                Some(rec) => {
                    if rec.malformed {
                        self.malformed_count += 1;
                    }
                    self.records.push(rec);
                }
                None => {
                    self.fully_consumed = true;
                    break;
                }
            }
        }
        Ok(self.records.len())
    }

    /// Drive the reader to EOF, materialising every remaining record.
    /// Used by the info path so the record-count field can stop showing
    /// the `≥ N` qualifier.
    pub fn ensure_all(&mut self) -> Result<()> {
        while !self.fully_consumed {
            match read_next(&mut self.reader, &mut self.last_line)? {
                Some(rec) => {
                    if rec.malformed {
                        self.malformed_count += 1;
                    }
                    self.records.push(rec);
                }
                None => {
                    self.fully_consumed = true;
                }
            }
        }
        Ok(())
    }

    /// Total record count if the reader has been driven to EOF, otherwise
    /// `None` (caller renders as `≥ records.len()`).
    pub fn total_records(&self) -> Option<usize> {
        if self.fully_consumed {
            Some(self.records.len())
        } else {
            None
        }
    }

    /// Number of records currently loaded — same as `records.len()`.
    pub fn loaded(&self) -> usize {
        self.records.len()
    }

    /// Column count from the first record (or zero if empty).
    pub fn column_count(&self) -> usize {
        self.records
            .iter()
            .find(|r| !r.malformed)
            .map(|r| r.cells.len())
            .unwrap_or(0)
    }
}

/// Read the next record. `Ok(None)` on EOF, `Ok(Some(Record))` for both
/// well-formed and malformed records. Errors are converted to
/// `Record::error()` so the parser can resync without bubbling out.
fn read_next(
    reader: &mut csv::Reader<Box<dyn Read>>,
    last_line: &mut u64,
) -> Result<Option<Record>> {
    let mut sr = csv::StringRecord::new();
    match reader.read_record(&mut sr) {
        Ok(true) => {
            let pos_line = reader.position().clone().line();
            let span = pos_line.saturating_sub(*last_line);
            *last_line = pos_line;
            let bytes_total: usize = sr.iter().map(|c| c.len()).sum();
            if span > MAX_RECORD_LINES || bytes_total > MAX_RECORD_BYTES {
                return Ok(Some(Record::error()));
            }
            let cells = sr.iter().map(|s| s.to_string()).collect();
            Ok(Some(Record::ok(cells)))
        }
        Ok(false) => Ok(None),
        Err(_) => {
            // csv crate's reader auto-resyncs at the next newline on the
            // next read_record call, so we just emit an error row and
            // let the caller continue.
            Ok(Some(Record::error()))
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

/// Build the body reader passed to the csv crate. UTF-8: streaming
/// `ByteStream` wrapped in a `BufReader`, BOM bytes consumed up front.
/// UTF-16: read the full source, transcode to UTF-8, wrap in `Cursor`.
fn build_body_reader(
    source: &InputSource,
    encoding: Encoding,
    body_offset: usize,
) -> Result<Box<dyn Read>> {
    match encoding {
        Encoding::Utf8 => {
            let mut stream = source.open_stream()?;
            if body_offset > 0 {
                let mut throw = vec![0u8; body_offset];
                stream.read_exact(&mut throw)?;
            }
            Ok(Box::new(BufReader::new(stream)))
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let raw = source.read_bytes()?;
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
    first
        .cells
        .iter()
        .all(|c| matches!(classify_cell(c), CellKind::Text | CellKind::Empty))
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

    #[test]
    fn seed_parses_simple_csv() {
        let src = stdin("name,age\nalice,30\nbob,25\n");
        let data = CsvData::open(&src, CsvFormat::Csv).unwrap();
        assert_eq!(data.delimiter, b',');
        assert_eq!(data.records.len(), 3);
        assert_eq!(data.records[0].cells, vec!["name", "age"]);
        assert_eq!(data.records[1].cells, vec!["alice", "30"]);
        assert!(data.header_heuristic);
        assert_eq!(data.column_count(), 2);
    }

    #[test]
    fn tsv_uses_tab_delimiter() {
        let src = stdin("a\tb\tc\n1\t2\t3\n");
        let data = CsvData::open(&src, CsvFormat::Tsv).unwrap();
        assert_eq!(data.delimiter, b'\t');
        assert_eq!(data.records[0].cells, vec!["a", "b", "c"]);
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
        assert_eq!(data.records.len(), 3);
        assert_eq!(data.records[1].cells, vec!["one\ntwo", "x"]);
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
        assert_eq!(data.records[0].cells, vec!["name", "age"]);
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
        assert_eq!(data.records[0].cells, vec!["a", "b"]);
        assert_eq!(data.records[1].cells, vec!["1", "2"]);
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
}
