//! Email Info sidecar — header summary + attachment / message tallies.

use crate::info::Extras;
use crate::input::InputSource;

use super::EmailFormat;
use super::message::ParsedEmail;
use super::{mbox, message};

/// Per-email metadata for the Info section. Single-message (`.eml`)
/// populates the header fields; mailbox (`.mbox`) populates
/// `message_count` and leaves the per-message fields empty.
pub struct EmailInfo {
    pub format: EmailFormat,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub subject: Option<String>,
    pub date: Option<String>,
    pub message_id: Option<String>,
    pub attachment_count: usize,
    pub attachment_bytes: u64,
    /// Message count for an mbox; `None` for a single `.eml`.
    pub message_count: Option<usize>,
}

impl EmailInfo {
    fn from_message(email: &ParsedEmail) -> Self {
        Self {
            format: EmailFormat::Eml,
            from: email.from.clone(),
            to: email.to.clone(),
            cc: email.cc.clone(),
            subject: email.subject.clone(),
            date: email.date.clone(),
            message_id: email.message_id.clone(),
            attachment_count: email.attachments.len(),
            attachment_bytes: email.attachments.iter().map(|a| a.size).sum(),
            message_count: None,
        }
    }

    fn mbox(message_count: usize) -> Self {
        Self {
            format: EmailFormat::Mbox,
            from: None,
            to: None,
            cc: None,
            subject: None,
            date: None,
            message_id: None,
            attachment_count: 0,
            attachment_bytes: 0,
            message_count: Some(message_count),
        }
    }
}

/// Collect the Info sidecar. `.mbox` streams the split to count messages
/// (no whole-file load); `.eml` parses the single message. Returns `None`
/// when the bytes don't parse as mail, so the gather falls back to
/// text/binary.
pub fn gather_extras(source: &InputSource, fmt: EmailFormat) -> Option<Extras> {
    let info = match fmt {
        EmailFormat::Eml => EmailInfo::from_message(&message::parse(
            &source
                .read_bytes(crate::input::limits::Budget::Sidecar("email"))
                .ok()?,
        )?),
        EmailFormat::Mbox => EmailInfo::mbox(mbox::split(source).ok()?.len()),
    };
    Some(Box::new(info))
}
