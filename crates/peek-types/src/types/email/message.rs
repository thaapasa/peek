//! Parsing wrapper over `mail-parser`. One [`parse`] call turns the raw
//! message bytes into an owned [`ParsedEmail`] — the single parse site
//! shared by the renderer, the Info gather, and the attachment extractor
//! (each re-parses its own source; a message is small enough that a
//! whole-message parse is cheap, and owning the result frees the caller
//! from `mail-parser`'s borrow lifetimes).

use std::time::{Duration, SystemTime};

use mail_parser::{Address, Message, MessageParser, MimeHeaders};

/// The display-facing view of one email message.
pub struct ParsedEmail {
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub subject: Option<String>,
    /// `Date:` header verbatim (RFC-822), for the faithful read-view header
    /// block. The Info section uses [`timestamp`](Self::timestamp) instead.
    pub date: Option<String>,
    /// `Date:` resolved to a wall-clock instant (`mail-parser` folds the
    /// RFC-822 zone to a UTC epoch). `None` when absent or at/before the epoch
    /// — the `> 0` guard doubles as a parse-failure / bogus-1970 filter.
    pub timestamp: Option<SystemTime>,
    pub message_id: Option<String>,
    /// Preferred renderable body — HTML when the message carries one,
    /// otherwise the plain-text part.
    pub body: Body,
    /// Extractable attachments (non-body parts), in document order.
    pub attachments: Vec<Attachment>,
}

pub enum Body {
    Html(String),
    Text(String),
    Empty,
}

pub struct Attachment {
    /// Stable listing-name / extract-key for this attachment.
    pub key: String,
    pub size: u64,
    /// `type/subtype` content type (`application/octet-stream` when the
    /// part declares none). Shown as the listing's content-type column.
    pub content_type: String,
}

/// Parse raw message bytes into the owned display view. Returns `None`
/// only when `mail-parser` cannot parse the bytes at all.
pub fn parse(bytes: &[u8]) -> Option<ParsedEmail> {
    let msg = MessageParser::default().parse(bytes)?;

    let html = msg
        .html_bodies()
        .next()
        .and_then(|p| p.text_contents().map(str::to_owned));
    let text = msg
        .text_bodies()
        .next()
        .and_then(|p| p.text_contents().map(str::to_owned));
    let body = match (html, text) {
        (Some(h), _) => Body::Html(h),
        (None, Some(t)) => Body::Text(t),
        (None, None) => Body::Empty,
    };

    let attachments = attachment_keys(&msg)
        .into_iter()
        .zip(msg.attachments())
        .map(|(key, part)| Attachment {
            key,
            size: part.len() as u64,
            content_type: content_type(part),
        })
        .collect();

    Some(ParsedEmail {
        from: msg.from().and_then(format_address),
        to: msg.to().and_then(format_address),
        cc: msg.cc().and_then(format_address),
        subject: msg.subject().map(str::to_owned),
        date: msg.date().map(|d| d.to_rfc822()),
        timestamp: msg
            .date()
            .map(|d| d.to_timestamp())
            .filter(|&s| s > 0)
            .map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s as u64)),
        message_id: msg.message_id().map(str::to_owned),
        body,
        attachments,
    })
}

/// The unique extract keys for a message's attachments, in document
/// order. The single derivation shared by [`parse`] (builds the listing
/// rows) and the extractor (matches a key back to its part) — so the two
/// can't drift and silently mismap a row to the wrong attachment.
pub(crate) fn attachment_keys(msg: &Message) -> Vec<String> {
    let bases: Vec<String> = msg
        .attachments()
        .enumerate()
        .map(|(i, p)| attachment_base(i, p.attachment_name(), &content_type(p)))
        .collect();
    dedupe_keys(&bases)
}

/// The base name for an attachment: the declared filename (leaf,
/// control-stripped) when present, else a synthesised `attachment-N.<ext>`.
/// Stays a plain filename — no index prefix — so the common case is
/// `peek --extract notes.txt mail.eml` with no quoting.
fn attachment_base(index: usize, filename: Option<&str>, content_type: &str) -> String {
    match filename.map(sanitize_name) {
        Some(name) if !name.is_empty() => name,
        _ => format!(
            "attachment-{}{}",
            index + 1,
            ext_for_content_type(content_type)
        ),
    }
}

/// Turn per-attachment base names into unique extract keys. Names that
/// are already unique pass through unchanged (clean CLI keys); a genuine
/// collision gets a `-N` suffix on the stem (`notes.txt` → `notes-2.txt`)
/// bumped until free, so every part stays individually addressable
/// without burdening the common case.
fn dedupe_keys(bases: &[String]) -> Vec<String> {
    let mut used = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(bases.len());
    for base in bases {
        if used.insert(base.clone()) {
            out.push(base.clone());
            continue;
        }
        let (stem, ext) = split_ext(base);
        let mut n = 2;
        let key = loop {
            let cand = format!("{stem}-{n}{ext}");
            if used.insert(cand.clone()) {
                break cand;
            }
            n += 1;
        };
        out.push(key);
    }
    out
}

/// Split a filename into `(stem, ext)` where `ext` includes the dot
/// (`"notes.txt"` → `("notes", ".txt")`). A leading-dot name or one with
/// no dot has an empty extension.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(pos) if pos > 0 => name.split_at(pos),
        _ => (name, ""),
    }
}

/// `text` form of a part's `Content-Type` (`type/subtype`), defaulting
/// to `application/octet-stream` when absent.
fn content_type(part: &mail_parser::MessagePart) -> String {
    match part.content_type() {
        Some(ct) => match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_string(),
        },
        None => "application/octet-stream".to_string(),
    }
}

/// Render an address header to a single display string, joining multiple
/// recipients with `, `. Prefers `Name <addr>`, falling back to either
/// alone.
fn format_address(addr: &Address) -> Option<String> {
    let parts: Vec<String> = addr
        .iter()
        .map(|a| match (a.name(), a.address()) {
            (Some(name), Some(email)) => format!("{name} <{email}>"),
            (Some(name), None) => name.to_string(),
            (None, Some(email)) => email.to_string(),
            (None, None) => String::new(),
        })
        .filter(|s| !s.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Strip directory components and control characters from a declared
/// attachment filename so it is safe to surface as a listing key. Full
/// path-traversal sanitising happens again at extract time.
fn sanitize_name(name: &str) -> String {
    let leaf = name.rsplit(['/', '\\']).next().unwrap_or(name);
    leaf.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

/// A best-effort file extension for a synthesised attachment name.
fn ext_for_content_type(content_type: &str) -> &'static str {
    match content_type {
        "text/plain" => ".txt",
        "text/html" => ".html",
        "application/pdf" => ".pdf",
        "application/json" => ".json",
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/gif" => ".gif",
        "application/zip" => ".zip",
        _ => ".bin",
    }
}
