//! Per-type compose for email.
//!
//! - `.eml`: a rendered message read view (headers + body), the raw
//!   RFC822 source, and — when the message carries attachments — an
//!   extractable attachments listing.
//! - `.mbox`: a message-list TOC whose rows drill into a single message
//!   (the rendered view above, over a zero-copy `subrange` of that one
//!   message), plus the raw mailbox source.

use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::{Detected, EmailFormat, FileType};
use crate::viewer::ComposeCtx;
use crate::viewer::ComposeOpts;
use crate::viewer::listing::{Entry, EntryKind, EntryMtime, ListingMode, time_from_epoch_secs};
use crate::viewer::modes::{DescendFrame, ExtractTarget, Mode, RenderedTextMode};

use super::message::{self, ParsedEmail};
use super::renderer::EmailRenderer;
use super::{EmailFormat as Fmt, mbox};

pub fn compose(
    source: &InputSource,
    detected: &Detected,
    args: &ComposeOpts,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: EmailFormat,
) -> Result<()> {
    match fmt {
        Fmt::Eml => {
            // Rendered view (unless --plain), then raw source, then the
            // attachments listing. Source precedes the listing so the
            // print/pipe "first data mode" pick is the message itself —
            // the rendered body normally, the raw RFC822 under --plain.
            if !ctx.plain_mode {
                modes.push(rendered_mode(source));
            }
            modes.push(ctx.text_content_mode(source, &FileType::Email(Fmt::Eml), args, None)?);
            if let Some(listing) = attachments_listing(source) {
                modes.push(Box::new(listing));
            }
        }
        Fmt::Mbox => {
            compose_mbox(source, detected, ctx.plain_mode, modes);
            modes.push(ctx.text_content_mode(source, &FileType::Email(Fmt::Mbox), args, None)?);
        }
    }
    Ok(())
}

/// The rendered message read view (headers + body).
fn rendered_mode(source: &InputSource) -> Box<dyn Mode> {
    Box::new(RenderedTextMode::new(EmailRenderer::new(source.clone())))
}

/// Build the attachments listing for a message, or `None` when it has no
/// attachments. Rows extract through the standard `e` pipeline
/// (`email::extract`).
fn attachments_listing(source: &InputSource) -> Option<ListingMode> {
    let bytes = source.read_bytes().ok()?;
    let email = message::parse(&bytes)?;
    if email.attachments.is_empty() {
        return None;
    }
    let entries = attachment_entries(&email);
    Some(ListingMode::new(
        "Email",
        "Attachments",
        entries,
        Vec::new(),
    ))
}

fn attachment_entries(email: &ParsedEmail) -> Vec<Entry> {
    email
        .attachments
        .iter()
        .map(|a| Entry {
            name: a.key.clone(),
            size: a.size,
            mtime: None,
            mode: None,
            kind: EntryKind::File,
        })
        .collect()
}

/// Build the mbox message-list TOC with a descend handler that opens the
/// selected message over a zero-copy `subrange` of the mailbox.
fn compose_mbox(
    source: &InputSource,
    detected: &Detected,
    plain: bool,
    modes: &mut Vec<Box<dyn Mode>>,
) {
    let messages = match mbox::split(source) {
        Ok(m) => m,
        Err(e) => {
            modes.push(Box::new(ListingMode::new(
                "Mbox",
                "Messages",
                Vec::new(),
                vec![format!("Failed to read mailbox: {e:#}")],
            )));
            return;
        }
    };
    // Each row's listing name carries its 1-based index so duplicate
    // subjects stay distinct and the descend handler can recover the
    // message position from the extract key.
    let rows: Vec<(String, u64, u64)> = messages
        .iter()
        .enumerate()
        .map(|(i, m)| (format!("{}. {}", i + 1, m.subject), m.offset, m.len))
        .collect();

    let entries = rows
        .iter()
        .zip(&messages)
        .map(|((name, _, len), m)| Entry {
            name: name.clone(),
            size: *len,
            mtime: m
                .date_secs
                .and_then(|s| time_from_epoch_secs(s as u64))
                .map(EntryMtime::Utc),
            mode: None,
            kind: EntryKind::File,
        })
        .collect();

    let descend_source = source.clone();
    let descend_detected = detected.clone();
    let listing = ListingMode::new("Mbox", "Messages", entries, Vec::new()).with_descend_handler(
        move |target| {
            let ExtractTarget::EntryPath(key) = target else {
                return None;
            };
            let (name, offset, len) = rows.iter().find(|(n, _, _)| n == key)?;
            Some(build_message_frame(
                &descend_source,
                &descend_detected,
                name,
                *offset,
                *len,
                plain,
            ))
        },
    );
    modes.push(Box::new(listing));
}

/// Construct a descend frame viewing one mbox message: a `subrange` of
/// the mailbox re-detected as a single `.eml`. Builds the same view stack
/// a standalone `.eml` gets — rendered message + attachments, then the
/// universal Hex / Info / About / Help tail — so the descended frame
/// doesn't drift from a top-level one.
pub(crate) fn build_message_frame(
    source: &InputSource,
    detected: &Detected,
    label: &str,
    offset: u64,
    len: u64,
    plain: bool,
) -> Result<DescendFrame> {
    let msg_source = source.subrange(offset, len, label.to_string());
    let mut msg_modes: Vec<Box<dyn Mode>> = Vec::new();
    if !plain {
        msg_modes.push(rendered_mode(&msg_source));
    }
    if let Some(listing) = attachments_listing(&msg_source) {
        msg_modes.push(Box::new(listing));
    }

    // The slice is a standalone single message regardless of the parent
    // detection, so the descended frame is a plain `.eml`.
    let msg_detected = Detected::new(FileType::Email(Fmt::Eml), detected.magic_mime.clone());
    // The subrange source *is* this one message, so hexing it is correct.
    crate::viewer::append_universal_modes(&mut msg_modes, Some(&msg_source))?;

    Ok(DescendFrame {
        source: msg_source,
        detected: msg_detected,
        modes: msg_modes,
        breadcrumb_label: Some(label.to_string()),
    })
}
