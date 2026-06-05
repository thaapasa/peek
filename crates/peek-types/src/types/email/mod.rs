//! Email (`.eml` single message / `.mbox` mailbox) viewer + info sidecar.
//!
//! Views composed for an `.eml`:
//! 1. Rendered message — a themed header block (From / To / Cc / Date /
//!    Subject) followed by the body. HTML bodies route through the shared
//!    html2text driver (`types::html::render`); plain-text bodies are
//!    word-wrapped. See [`renderer`].
//! 2. Source — the raw RFC822 text via the generic content mode.
//! 3. Attachments (when present) — a `ListingMode` over the message's
//!    MIME attachments; `e` extracts one via [`extract`].
//!
//! An `.mbox` shows a message-list TOC instead: each row drills (via the
//! listing's descend handler) into a single message over a zero-copy
//! `InputSource::subrange`, reusing the `.eml` message views.
//!
//! All message parsing funnels through [`message::parse`]; mbox splitting
//! is hand-rolled in [`mbox`] (mail-parser parses single messages), which
//! also yields the byte offsets the subrange descend relies on.

pub mod compose;
pub mod extract;
pub mod info;
pub mod info_render;
mod mbox;
mod message;
mod renderer;

#[cfg(test)]
mod tests;

pub use info::EmailInfo;

/// Format enum, re-exported from `peek_detect` at the module root so
/// reader code keeps a local `crate::types::email::EmailFormat` path.
pub use peek_detect::types::email::EmailFormat;
