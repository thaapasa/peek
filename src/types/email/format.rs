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
