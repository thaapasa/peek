//! Email format flavour: single message (`.eml`) vs mailbox (`.mbox`).

/// Which email container peek is looking at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmailFormat {
    /// A single RFC822 / MIME message (`.eml`).
    Eml,
    /// A concatenation of messages separated by `From ` lines (`.mbox`).
    Mbox,
}

impl EmailFormat {
    /// Human-facing label used as the listing format name and Info row.
    pub fn label(self) -> &'static str {
        match self {
            EmailFormat::Eml => "Email",
            EmailFormat::Mbox => "Mbox",
        }
    }
}

// Email detection: extension and content sniff.
//
// `.eml` is a single RFC822 message; `.mbox` is a concatenation of
// messages each preceded by a `From ` separator line. Content sniffing
// distinguishes the two and guards against false positives (a stray
// `Key: value` line in a source file is not an email — we require a
// recognised mail header near the top).

/// Map a lowercase extension to an email format.
pub fn format_from_ext(ext: &str) -> Option<EmailFormat> {
    match ext {
        "eml" => Some(EmailFormat::Eml),
        "mbox" => Some(EmailFormat::Mbox),
        _ => None,
    }
}

/// Header names whose presence near the top strongly implies a mail
/// message (as opposed to an arbitrary `Key: value` text file).
const MAIL_HEADERS: &[&str] = &[
    "from",
    "to",
    "cc",
    "subject",
    "date",
    "received",
    "message-id",
    "mime-version",
    "return-path",
    "delivered-to",
    "content-type",
];

/// Sniff a UTF-8 head for an RFC822 message or an mbox mailbox. Returns
/// the format when the leading lines look like mail, else `None`.
pub fn sniff_text(text: &str) -> Option<EmailFormat> {
    let head = text.trim_start_matches(['\r', '\n']);

    // mbox: the very first line is a `From ` separator (From + space,
    // distinct from the `From:` header). The body that follows is itself
    // an RFC822 message, so require at least one mail header after it to
    // avoid matching prose that happens to begin with "From ".
    if head.starts_with("From ") {
        let first_line = head.lines().next().unwrap_or("");
        // git format-patch output borrows the mbox `From ` line shape
        // (`From <40-hex-sha> Mon Sep 17 00:00:00 2001`). Reject it so a
        // `.patch` / `.diff` falls through to source highlighting instead
        // of opening as a one-row mailbox.
        if is_git_patch_separator(first_line) {
            return None;
        }
        let after_sep = head.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
        return looks_like_headers(after_sep).then_some(EmailFormat::Mbox);
    }

    looks_like_headers(head).then_some(EmailFormat::Eml)
}

/// Whether a `From ` line is git's format-patch pseudo-separator rather
/// than a real mbox envelope. Git emits `From <commit-sha> Mon Sep 17
/// 00:00:00 2001`: a hex commit id (40 hex for SHA-1, 64 for SHA-256) and
/// a fixed sentinel date — never an envelope address.
fn is_git_patch_separator(from_line: &str) -> bool {
    if from_line.contains("Mon Sep 17 00:00:00 2001") {
        return true;
    }
    from_line.split_whitespace().nth(1).is_some_and(|tok| {
        matches!(tok.len(), 40 | 64) && tok.bytes().all(|b| b.is_ascii_hexdigit())
    })
}

/// Whether the leading lines form an RFC822 header block: a run of
/// `Name: value` (with continuation lines) containing at least one
/// recognised mail header, terminated by a blank line.
fn looks_like_headers(text: &str) -> bool {
    let mut saw_mail_header = false;
    let mut saw_any_header = false;
    for (i, line) in text.lines().enumerate() {
        if line.is_empty() {
            // Blank line ends the header block — valid only once we've
            // seen real headers.
            return saw_mail_header;
        }
        // Folded continuation line (leading whitespace) belongs to the
        // previous header.
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((name, _)) = line.split_once(':') else {
            // A non-header, non-continuation line in the block → not mail.
            return false;
        };
        if name.is_empty() || !name.bytes().all(is_header_name_byte) {
            return false;
        }
        saw_any_header = true;
        let lower = name.to_ascii_lowercase();
        if MAIL_HEADERS.contains(&lower.as_str()) {
            saw_mail_header = true;
        }
        // Bound the scan: real header blocks are not thousands of lines.
        if i >= 64 {
            break;
        }
    }
    saw_any_header && saw_mail_header
}

/// RFC5322 header field-name characters: printable ASCII except colon.
fn is_header_name_byte(b: u8) -> bool {
    b.is_ascii_graphic() && b != b':'
}
