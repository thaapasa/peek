//! RTF parse + AST conversion.
//!
//! The on-the-wire structure of an RTF file is a tree of brace
//! groups. Real-world Word RTFs embed cover images and OLE objects
//! as `{\pict … <hex-encoded bytes>}` groups inline with the prose;
//! `rtf_parser` doesn't strip those, so without preprocessing the
//! hex blob falls through to the rendered body and floods the read
//! view. The preprocessing step below:
//!
//! - Lifts the `\info` metadata group (rtf_parser also doesn't
//!   strip that).
//! - Catalogs and removes every `\pict` / `\object` / `\bin`
//!   destination group, recording the embedded blob (kind + decoded
//!   size + raw hex) so the listing view can surface them as files.
//! - Injects an explicit `\\\n` escape after every `\par` so
//!   rtf_parser's lexer emits a `Token::CRLF` (it doesn't otherwise
//!   tokenise `\par`).
//!
//! After that pass the rest of the body goes through `rtf_parser`
//! and the resulting `StyleBlock`s are mirrored into our owned
//! [`Block`] AST with theme-friendly painter colors.

use anyhow::{Context, Result, anyhow};
use rtf_parser::{Color as RtfColor, Painter, Paragraph as RtfParagraph, RtfDocument};

use crate::input::InputSource;
use crate::types::document::DocumentMetadata;
use crate::viewer::listing::{Entry, EntryKind};

pub(crate) struct Parsed {
    pub metadata: DocumentMetadata,
    pub blocks: Vec<Block>,
    pub paragraph_count: usize,
    pub word_count: usize,
    pub embeds: Vec<Embed>,
}

#[derive(Clone)]
pub(crate) struct Block {
    pub painter: BlockPainter,
    /// Paragraph alignment / spacing as parsed by `rtf_parser`. Carried
    /// for future justification rendering; unused by the v1 renderer.
    #[allow(dead_code)]
    pub paragraph: RtfParagraph,
    pub text: String,
}

#[derive(Clone, Default)]
pub(crate) struct BlockPainter {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<[u8; 3]>,
}

/// One inline-encoded embedded resource (image / OLE blob) lifted
/// out of the body during preprocessing. `name` is a synthesised
/// path like `image1.jpg` so the listing view + extract dispatch
/// can address it the same way as a ZIP entry. `hex` is the raw
/// hex-encoded byte sequence as it appeared in the source.
#[derive(Clone)]
pub(crate) struct Embed {
    pub name: String,
    /// Detected payload format. Recorded so future code (e.g. an
    /// info-section breakdown) can show jpeg vs png counts; the
    /// extension already carries the same info into the listing.
    #[allow(dead_code)]
    pub kind: EmbedKind,
    pub hex: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EmbedKind {
    Jpeg,
    Png,
    Emf,
    Wmf,
    MacPict,
    /// Unknown / unrecognised picture or object payload.
    Other,
}

impl EmbedKind {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Emf => "emf",
            Self::Wmf => "wmf",
            Self::MacPict => "pict",
            Self::Other => "bin",
        }
    }
}

impl Embed {
    /// Decode the hex blob into raw bytes, ignoring any whitespace
    /// embedded by line wrapping in the source.
    pub fn decode_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.hex.len() / 2);
        let mut hi: Option<u8> = None;
        for ch in self.hex.chars() {
            let Some(d) = hex_digit(ch) else {
                continue;
            };
            match hi {
                None => hi = Some(d),
                Some(h) => {
                    out.push((h << 4) | d);
                    hi = None;
                }
            }
        }
        out
    }

    /// Decoded byte length without materialising the bytes.
    pub fn decoded_len(&self) -> u64 {
        self.hex.chars().filter(|c| hex_digit(*c).is_some()).count() as u64 / 2
    }
}

/// Replace every `\par` control word (with its proper end-of-word
/// delimiter) with `\par\\\n` — the trailing `\\\n` is the explicit
/// escape-newline that `rtf_parser`'s lexer turns into a `Token::CRLF`.
///
/// The naive `str::replace("\\par", "\\par\\\n")` would also match
/// the prefix of `\pard`, `\pardirnatural`, `\paragraph`, etc. —
/// every such match injected a `\\\n` mid-word and the trailing
/// alphabetic character of the original control word leaked into
/// the rendered body as plain text (a literal `d`, `i`, etc.).
fn inject_par_breaks(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + text.len() / 16);
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"\\par" {
            let end_of_word = bytes
                .get(i + 4)
                .map(|b| !b.is_ascii_alphanumeric())
                .unwrap_or(true);
            if end_of_word {
                out.push_str("\\par\\\n");
                i += 4;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Build a flat listing-mode tree from a list of embeds. One entry
/// per embed; no directory nesting (RTF embeds aren't path-keyed).
pub(crate) fn embeds_to_entries(embeds: &[Embed]) -> Vec<Entry> {
    embeds
        .iter()
        .map(|e| Entry {
            name: e.name.clone(),
            size: e.decoded_len(),
            mtime: None,
            mode: None,
            kind: EntryKind::File,
        })
        .collect()
}

fn hex_digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

/// Parse RTF bytes from an [`InputSource`].
pub(crate) fn open_source(source: &InputSource) -> Result<Parsed> {
    let bytes = source.read_bytes().context("failed to read RTF source")?;
    let text =
        std::str::from_utf8(&bytes).map_err(|e| anyhow!("RTF body must be ASCII / UTF-8: {e}"))?;
    open_str(text)
}

pub(crate) fn open_str(text: &str) -> Result<Parsed> {
    let metadata = scan_info_group(text);
    let stripped = strip_info_group(text);

    // Lift binary blobs (`\pict`, `\object`) out of the source
    // before the lexer sees them. Each removed group becomes one
    // entry in `embeds`; the rest of the source flows through
    // `rtf_parser` for body rendering.
    let (preprocessed_no_blobs, embeds) = strip_destination_groups(&stripped);

    // rtf_parser doesn't tokenise `\par`; inject a synthetic
    // escape-newline after each so paragraph breaks become CRLFs
    // the parser then emits as `\n` in body text. Word-boundary
    // matched so `\pard` (paragraph default) and `\paragraph`-style
    // longer control words aren't rewritten.
    let preprocessed = inject_par_breaks(&preprocessed_no_blobs);

    let doc = RtfDocument::try_from(preprocessed.as_str())
        .map_err(|e| anyhow!("RTF parse failed: {e}"))?;
    let blocks: Vec<Block> = doc
        .body
        .iter()
        .map(|sb| Block {
            painter: convert_painter(&sb.painter, &doc.header.color_table),
            paragraph: sb.paragraph,
            text: sb.text.clone(),
        })
        .collect();

    let paragraph_count = blocks
        .iter()
        .map(|b| b.text.matches('\n').count())
        .sum::<usize>()
        .max(if blocks.is_empty() { 0 } else { 1 });
    let word_count = blocks
        .iter()
        .flat_map(|b| b.text.split_whitespace())
        .filter(|w| !w.is_empty())
        .count();

    Ok(Parsed {
        metadata,
        blocks,
        paragraph_count,
        word_count,
        embeds,
    })
}

fn convert_painter(
    src: &Painter,
    color_table: &std::collections::HashMap<rtf_parser::ColorRef, RtfColor>,
) -> BlockPainter {
    // Drop near-grayscale colors. Authoring tools routinely emit
    // `\cf1` referring to a "Black" color-table slot to mark "use
    // default text color"; emitting that as a literal `#000000` SGR
    // would fight the user's terminal foreground (black-on-black on
    // dark themes). Saturated accents (`max - min > 24`) still
    // render. Mirrors `types/html/render.rs::is_grayscale`.
    let color = match src.color_ref {
        0 => None,
        r => color_table.get(&r).and_then(|c| {
            let max = c.red.max(c.green).max(c.blue);
            let min = c.red.min(c.green).min(c.blue);
            if max.saturating_sub(min) < 24 {
                None
            } else {
                Some([c.red, c.green, c.blue])
            }
        }),
    };
    BlockPainter {
        bold: src.bold,
        italic: src.italic,
        underline: src.underline,
        strike: src.strike,
        color,
    }
}

// ---------------------------------------------------------------------------
// `\info` group — title / creator / subject / dates
// ---------------------------------------------------------------------------

fn scan_info_group(text: &str) -> DocumentMetadata {
    let mut out = DocumentMetadata::default();
    let Some((info_open, info_close)) = info_group_bounds(text) else {
        return out;
    };
    let info = &text[info_open..info_close];

    out.title = scan_tag(info, "title");
    out.creator = scan_tag(info, "author");
    out.subject = scan_tag(info, "subject");
    out.keywords = scan_tag(info, "keywords");
    out.created = scan_date_tag(info, "creatim");
    out.modified = scan_date_tag(info, "revtim");
    out
}

fn strip_info_group(text: &str) -> String {
    let Some((info_open, info_close)) = info_group_bounds(text) else {
        return text.to_string();
    };
    let info_word_start = info_open - b"\\info".len();
    let group_start = text[..info_word_start]
        .rfind('{')
        .unwrap_or(info_word_start);
    let group_end = (info_close + 1).min(text.len());
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..group_start]);
    out.push_str(&text[group_end..]);
    out
}

fn info_group_bounds(text: &str) -> Option<(usize, usize)> {
    let info_start = text.find("\\info")?;
    let bytes = text.as_bytes();
    let mut depth = 1i32;
    let mut i = info_start + b"\\info".len();
    let info_open = i;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((info_open, i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn scan_tag(info: &str, tag: &str) -> Option<String> {
    let needle = format!("{{\\{tag} ");
    let start = info.find(&needle)? + needle.len();
    let rest = &info[start..];
    let end = rest.find('}')?;
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn scan_date_tag(info: &str, tag: &str) -> Option<String> {
    let needle = format!("{{\\{tag}");
    let start = info.find(&needle)? + needle.len();
    let rest = &info[start..];
    let end = rest.find('}')?;
    let body = &rest[..end];
    let yr = scan_int(body, "yr")?;
    if yr <= 0 {
        return None;
    }
    let mo = scan_int(body, "mo").unwrap_or(1);
    let dy = scan_int(body, "dy").unwrap_or(1);
    let hr = scan_int(body, "hr").unwrap_or(0);
    let mn = scan_int(body, "min").unwrap_or(0);
    Some(format!("{yr:04}-{mo:02}-{dy:02} {hr:02}:{mn:02}"))
}

fn scan_int(body: &str, tag: &str) -> Option<i32> {
    let needle = format!("\\{tag}");
    let start = body.find(&needle)? + needle.len();
    let rest = &body[start..];
    let mut digits = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_digit() || (digits.is_empty() && ch == '-') {
            digits.push(ch);
        } else {
            break;
        }
    }
    digits.parse().ok()
}

// ---------------------------------------------------------------------------
// Destination-group stripper
// ---------------------------------------------------------------------------

/// Walk the source byte-by-byte and excise every `{\pict … }` and
/// `{\object … }` group. Returns the cleaned source plus a list of
/// the embeds we removed (one entry per group), in document order
/// with auto-generated names like `image1.jpg`.
fn strip_destination_groups(text: &str) -> (String, Vec<Embed>) {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut embeds: Vec<Embed> = Vec::new();
    let mut copy_from = 0usize;
    let mut i = 0usize;

    while i < len {
        if bytes[i] != b'{' {
            i += 1;
            continue;
        }
        // Look at the first control word inside this group; allow an
        // optional `\*` ignorable-destination marker.
        let mut probe = i + 1;
        if probe + 1 < len && bytes[probe] == b'\\' && bytes[probe + 1] == b'*' {
            probe += 2;
            while probe < len && bytes[probe] == b'\\' {
                probe += 1;
            }
            // After `\*`, we expect another `\name`.
            if probe < len && bytes[probe] != b'\\' {
                probe = probe.saturating_sub(1);
            }
        }
        // Skip ASCII whitespace between `{` and the control word.
        while probe < len && bytes[probe].is_ascii_whitespace() {
            probe += 1;
        }
        if probe >= len || bytes[probe] != b'\\' {
            i += 1;
            continue;
        }
        // Read the control word identifier.
        let word_start = probe + 1;
        let mut word_end = word_start;
        while word_end < len && bytes[word_end].is_ascii_alphabetic() {
            word_end += 1;
        }
        let word = &text[word_start..word_end];
        let kind = match word {
            "pict" => Some(EmbedKind::Other),
            "object" => Some(EmbedKind::Other),
            _ => None,
        };
        let Some(initial_kind) = kind else {
            i += 1;
            continue;
        };
        // Walk forward to find the matching closing `}`. Track `\\`
        // escapes (skip the next byte) so braces inside escape
        // sequences don't desync the depth count.
        let group_start = i;
        let mut j = i + 1;
        let mut depth = 1i32;
        while j < len {
            match bytes[j] {
                b'\\' => {
                    j += 2;
                    continue;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if j >= len {
            // Unterminated group — bail and copy the rest as-is.
            break;
        }
        let group_end = j + 1;
        let group_body = &text[i + 1..j];

        // Refine kind from the inner control words.
        let kind = refine_embed_kind(group_body, initial_kind);
        let hex = scrape_hex_blob(group_body);
        let idx = embeds.len() + 1;
        let name = format!("{}{}.{}", embed_basename(kind), idx, kind.ext());
        embeds.push(Embed { name, kind, hex });

        // Copy [copy_from .. group_start) into out, then skip the
        // whole group.
        out.push_str(&text[copy_from..group_start]);
        copy_from = group_end;
        i = group_end;
    }
    out.push_str(&text[copy_from..len]);
    (out, embeds)
}

fn refine_embed_kind(body: &str, default: EmbedKind) -> EmbedKind {
    if body.contains("\\jpegblip") {
        EmbedKind::Jpeg
    } else if body.contains("\\pngblip") {
        EmbedKind::Png
    } else if body.contains("\\emfblip") {
        EmbedKind::Emf
    } else if body.contains("\\wmetafile") {
        EmbedKind::Wmf
    } else if body.contains("\\macpict") {
        EmbedKind::MacPict
    } else {
        default
    }
}

fn embed_basename(kind: EmbedKind) -> &'static str {
    match kind {
        EmbedKind::Jpeg | EmbedKind::Png | EmbedKind::Emf | EmbedKind::Wmf | EmbedKind::MacPict => {
            "image"
        }
        EmbedKind::Other => "object",
    }
}

/// Pull every hex digit out of an embed group body. Tabs / spaces /
/// newlines / control words are ignored. Nested groups (e.g. the
/// `{\*\picprop ...}` block at the head of a `\pict`) are skipped
/// over so only the primary hex blob is captured.
fn scrape_hex_blob(body: &str) -> String {
    let mut out = String::new();
    let mut in_control = false;
    let mut depth = 0i32;
    for ch in body.chars() {
        // Structural chars are matched first so they always take
        // effect — nested-group depth tracking has to win over the
        // control-word / hex branches.
        match ch {
            '\\' => {
                in_control = true;
                continue;
            }
            '{' => {
                depth += 1;
                continue;
            }
            '}' => {
                depth -= 1;
                if depth < 0 {
                    break;
                }
                continue;
            }
            _ => {}
        }
        if in_control {
            // Inside a control word identifier. Stay in this state
            // until we hit a non-alphanumeric (and non-`-`) char,
            // which terminates the control word per RTF spec. None
            // of these chars contribute to the hex blob even when
            // they happen to be valid hex digits (`a`..`f`).
            if !ch.is_ascii_alphanumeric() && ch != '-' {
                in_control = false;
            }
            continue;
        }
        if depth > 0 {
            // Skip nested-group content (`\*\picprop` etc.).
            continue;
        }
        if ch.is_ascii_hexdigit() {
            out.push(ch);
        }
    }
    out
}
