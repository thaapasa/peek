//! Mbox splitting. An mbox file concatenates messages, each preceded by
//! a `From ` separator line (the "From_" line, distinct from the `From:`
//! header). The splitter streams the source one line at a time via the
//! [`ByteStream`](peek_io::stream::ByteStream) `BufRead`, tracking byte
//! offsets and doing a lightweight Subject/Date header scan inline — it
//! never holds more than a single line, so even a multi-GB mailbox lists
//! without loading. Each [`MboxEntry`] points at the message *body*
//! (past the separator line) so the range parses as a standalone RFC822
//! message via `InputSource::subrange`.
//!
//! Note: separator detection is the lenient mboxo rule (any body line
//! beginning `From ` starts a new message). Generators that don't escape
//! such body lines (mboxrd does) can be mis-split; this matches the
//! ambiguity inherent to the format and what most readers do.

use std::io::BufRead;

use anyhow::Result;

use peek_io::InputSource;

/// One message located inside an mbox file.
pub struct MboxEntry {
    /// Byte offset of the message (just past its `From ` separator line).
    pub offset: u64,
    /// Byte length of the message.
    pub len: u64,
    /// Subject header for the listing row (`(no subject)` when absent).
    pub subject: String,
    /// Unix epoch seconds from the `Date` header, when present and valid.
    pub date_secs: Option<i64>,
}

/// Accumulates the header scan for the message currently being read.
struct Pending {
    body_start: u64,
    in_headers: bool,
    subject: Option<String>,
    date_secs: Option<i64>,
}

/// Stream the source and split it into per-message ranges. A separator is
/// a line beginning with `From ` at column 0 (file start, or right after
/// a newline). Returns an empty vec when no separator is present.
pub fn split(source: &InputSource) -> Result<Vec<MboxEntry>> {
    let mut stream = source.open_stream()?;
    let mut entries = Vec::new();
    let mut current: Option<Pending> = None;
    let mut line_start = 0u64;
    let mut line = Vec::new();

    loop {
        line.clear();
        let n = stream.read_until(b'\n', &mut line)? as u64;
        if n == 0 {
            break;
        }
        let next = line_start + n;

        if line.starts_with(b"From ") {
            // Separator: finalise the message that ended here, then begin
            // a new one whose body starts after this separator line.
            if let Some(p) = current.take() {
                push_entry(&mut entries, p, line_start);
            }
            current = Some(Pending {
                body_start: next,
                in_headers: true,
                subject: None,
                date_secs: None,
            });
        } else if let Some(p) = current.as_mut()
            && p.in_headers
        {
            scan_header_line(p, &line);
        }

        line_start = next;
    }
    if let Some(p) = current.take() {
        push_entry(&mut entries, p, line_start);
    }
    Ok(entries)
}

/// Finalise a pending message ending at `end`, pushing it unless empty.
fn push_entry(entries: &mut Vec<MboxEntry>, p: Pending, end: u64) {
    if end <= p.body_start {
        return;
    }
    entries.push(MboxEntry {
        offset: p.body_start,
        len: end - p.body_start,
        subject: p.subject.unwrap_or_else(|| "(no subject)".to_string()),
        date_secs: p.date_secs,
    });
}

/// Update a message's header scan from one raw line (with trailing
/// newline). A blank line ends the header block.
fn scan_header_line(p: &mut Pending, raw: &[u8]) {
    let line = raw.strip_suffix(b"\n").unwrap_or(raw);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.is_empty() {
        p.in_headers = false;
        return;
    }
    if p.subject.is_none()
        && let Some(rest) = strip_prefix_ci(line, b"subject:")
    {
        let value = String::from_utf8_lossy(rest).trim().to_string();
        if !value.is_empty() {
            p.subject = Some(value);
        }
    } else if p.date_secs.is_none()
        && let Some(rest) = strip_prefix_ci(line, b"date:")
    {
        let value = String::from_utf8_lossy(rest);
        // `> 0` deliberately drops the epoch/pre-epoch range: it's only
        // the listing mtime hint, and the cutoff doubles as a guard
        // against a parse failure surfacing as a bogus 1970 timestamp.
        p.date_secs = mail_parser::DateTime::parse_rfc822(value.trim())
            .map(|d| d.to_timestamp())
            .filter(|&s| s > 0);
    }
}

/// Case-insensitive ASCII prefix strip.
fn strip_prefix_ci<'a>(haystack: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if haystack.len() < prefix.len() {
        return None;
    }
    let (head, tail) = haystack.split_at(prefix.len());
    head.eq_ignore_ascii_case(prefix).then_some(tail)
}
