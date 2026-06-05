//! Unit tests for the email type: detection, parsing, mbox splitting,
//! and attachment extraction. Fixtures live in `test-data/`.

use crate::input::InputSource;

use crate::input::detect::{Detected, FileType};
use crate::viewer::modes::ModeId;

use peek_detect::types::email as detect;

use super::EmailFormat;
use super::{compose, extract, info, mbox, message};

const EML: &[u8] = include_bytes!("../../../../../test-data/sample.eml");
const MBOX: &[u8] = include_bytes!("../../../../../test-data/sample.mbox");

fn eml_str() -> &'static str {
    std::str::from_utf8(EML).unwrap()
}

#[test]
fn ext_maps_to_format() {
    assert_eq!(detect::format_from_ext("eml"), Some(EmailFormat::Eml));
    assert_eq!(detect::format_from_ext("mbox"), Some(EmailFormat::Mbox));
    assert_eq!(detect::format_from_ext("txt"), None);
}

#[test]
fn sniffs_eml_by_content() {
    assert_eq!(detect::sniff_text(eml_str()), Some(EmailFormat::Eml));
}

#[test]
fn sniffs_mbox_by_content() {
    let text = std::str::from_utf8(MBOX).unwrap();
    assert_eq!(detect::sniff_text(text), Some(EmailFormat::Mbox));
}

#[test]
fn does_not_sniff_arbitrary_colon_lines() {
    // A YAML-ish / source file with `key: value` lines but no mail
    // headers must not be mistaken for an email.
    let text = "name: peek\nversion: 1\ndescription: a file viewer\n";
    assert_eq!(detect::sniff_text(text), None);
}

#[test]
fn does_not_sniff_git_patch() {
    // git format-patch borrows the mbox `From ` line shape; it must not
    // classify as a mailbox (it routes to source/diff highlighting).
    let patch = "From 1234567890abcdef1234567890abcdef12345678 Mon Sep 17 00:00:00 2001\n\
        From: Dev <dev@example.com>\n\
        Date: Sun, 31 May 2026 10:00:00 +0000\n\
        Subject: [PATCH] fix the thing\n\n\
        diff --git a/x.rs b/x.rs\n";
    assert_eq!(detect::sniff_text(patch), None);
}

#[test]
fn parses_headers_and_prefers_html_body() {
    let email = message::parse(EML).expect("parse");
    assert_eq!(
        email.from.as_deref(),
        Some("Alice Example <alice@example.com>")
    );
    assert_eq!(email.to.as_deref(), Some("Bob Tester <bob@example.org>"));
    assert_eq!(
        email.subject.as_deref(),
        Some("Project Peek — sample message")
    );
    assert!(email.message_id.as_deref() == Some("sample-0001@example.com"));
    // Both alternatives exist; the HTML one wins.
    match &email.body {
        message::Body::Html(h) => assert!(h.contains("<h1>")),
        other => panic!(
            "expected HTML body, got {:?}",
            std::mem::discriminant(other)
        ),
    }
    assert_eq!(email.attachments.len(), 1);
    // A unique filename is the key verbatim — clean for CLI extraction.
    assert_eq!(email.attachments[0].key, "notes.txt");
}

#[test]
fn mbox_splits_into_messages() {
    let source = InputSource::memory(MBOX.to_vec(), "sample.mbox");
    let entries = mbox::split(&source).expect("split");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].subject, "First message");
    assert_eq!(entries[1].subject, "Re: First message");
    assert_eq!(entries[2].subject, "Third and final");
    // Date header parses to epoch seconds for the listing mtime column.
    assert!(entries.iter().all(|e| e.date_secs.is_some()));
    assert!(entries[0].date_secs.unwrap() < entries[1].date_secs.unwrap());
    // Each range parses as a standalone message.
    for e in &entries {
        let slice = &MBOX[e.offset as usize..(e.offset + e.len) as usize];
        let msg = message::parse(slice).expect("submessage parses");
        assert!(msg.subject.is_some());
    }
}

#[test]
fn extracts_attachment_by_key() {
    let source = InputSource::memory(EML.to_vec(), "sample.eml");
    let extracted = extract::extract(&source, "notes.txt").expect("extract");
    assert_eq!(extracted.suggested_name, "notes.txt");
    let bytes = extracted.source.read_bytes().expect("read");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("attached notes"));
}

#[test]
fn extract_unknown_key_is_not_found() {
    let source = InputSource::memory(EML.to_vec(), "sample.eml");
    assert!(extract::extract(&source, "nope.bin").is_err());
}

/// Two attachments declaring the same filename get distinct, individually
/// extractable keys (regression: the second was previously unreachable).
#[test]
fn duplicate_attachment_filenames_stay_distinct() {
    let raw = "From: a@b.c\r\nSubject: dup\r\nMIME-Version: 1.0\r\n\
        Content-Type: multipart/mixed; boundary=\"B\"\r\n\r\n\
        --B\r\nContent-Type: text/plain\r\n\
        Content-Disposition: attachment; filename=\"dup.txt\"\r\n\r\nfirst\r\n\
        --B\r\nContent-Type: text/plain\r\n\
        Content-Disposition: attachment; filename=\"dup.txt\"\r\n\r\nsecond\r\n\
        --B--\r\n";
    let email = message::parse(raw.as_bytes()).expect("parse");
    assert_eq!(email.attachments.len(), 2);
    // First keeps the clean name; the collision gets a `-N` stem suffix.
    assert_eq!(email.attachments[0].key, "dup.txt");
    assert_eq!(email.attachments[1].key, "dup-2.txt");

    // Both keys round-trip to the right part — parse and extract derive
    // keys through the same helper, so each maps to its own contents.
    let source = InputSource::memory(raw.as_bytes().to_vec(), "dup.eml");
    let first = extract::extract(&source, "dup.txt").expect("extract first");
    assert!(String::from_utf8_lossy(&first.source.read_bytes().unwrap()).contains("first"));
    let second = extract::extract(&source, "dup-2.txt").expect("extract second");
    assert_eq!(second.suggested_name, "dup-2.txt");
    assert!(String::from_utf8_lossy(&second.source.read_bytes().unwrap()).contains("second"));
}

#[test]
fn gather_eml_info() {
    let source = InputSource::memory(EML.to_vec(), "sample.eml");
    let extras = info::gather_extras(&source, EmailFormat::Eml).expect("gather");
    let i = crate::info::downcast_extras::<info::EmailInfo>(&extras);
    assert_eq!(i.message_count, None);
    assert_eq!(i.attachment_count, 1);
    assert_eq!(i.subject.as_deref(), Some("Project Peek — sample message"));
}

/// A descended mbox message gets the same full view stack as a
/// standalone `.eml` — rendered message + the universal Hex / Info / Help
/// tail — not a stripped-down frame.
#[test]
fn mbox_message_frame_has_full_view_stack() {
    let source = InputSource::memory(MBOX.to_vec(), "sample.mbox");
    let detected = Detected::new(FileType::Email(EmailFormat::Mbox), None);
    let entries = mbox::split(&source).expect("split");
    let e = &entries[0];
    let frame = compose::build_message_frame(
        &source,
        &detected,
        "1. First message",
        e.offset,
        e.len,
        false,
    )
    .expect("frame");
    let ids: Vec<ModeId> = frame.modes.iter().map(|m| m.id()).collect();
    for want in [ModeId::Rendered, ModeId::Hex, ModeId::Info, ModeId::Help] {
        assert!(
            ids.contains(&want),
            "descend frame missing {want:?}: {ids:?}"
        );
    }
}

#[test]
fn gather_mbox_info() {
    let source = InputSource::memory(MBOX.to_vec(), "sample.mbox");
    let extras = info::gather_extras(&source, EmailFormat::Mbox).expect("gather");
    let i = crate::info::downcast_extras::<info::EmailInfo>(&extras);
    assert_eq!(i.message_count, Some(3));
}
