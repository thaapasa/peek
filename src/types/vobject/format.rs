//! vObject flavour: iCalendar (`.ics`) vs vCard (`.vcf`).
//!
//! Both are IETF "vObject" text formats sharing one content-line grammar
//! (RFC 5545 §3.1 / RFC 6350 §3.3); only the component vocabulary and the
//! rendered presentation differ. The flavour picks which renderer the
//! compose stack wires and which Info summary the gather produces.

/// Which vObject document peek is looking at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VObjectFormat {
    /// iCalendar calendar (`.ics`) — events / todos inside a `VCALENDAR`.
    ICal,
    /// vCard address book (`.vcf`) — one or more `VCARD` contacts.
    VCard,
}

impl VObjectFormat {
    /// Human-facing label used as the Info section heading and rendered
    /// view title.
    pub fn label(self) -> &'static str {
        match self {
            VObjectFormat::ICal => "iCalendar",
            VObjectFormat::VCard => "vCard",
        }
    }
}
