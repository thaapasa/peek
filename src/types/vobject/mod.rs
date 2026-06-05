//! iCalendar (`.ics`) + vCard (`.vcf`) viewer + info sidecar.
//!
//! The two IETF "vObject" text formats share one content-line grammar
//! (RFC 5545 §3.1 / RFC 6350 §3.3), so they share the hand-rolled
//! [`line`] parser — content-line unfolding + a `BEGIN`/`END` component
//! tree — and diverge only at the renderer:
//!
//! * [`calendar`] turns a `VCALENDAR` into a readable agenda (events +
//!   todos, with humanised recurrence and date-time formatting).
//! * [`contact`] turns each `VCARD` into a grouped contact card.
//!
//! Each flavour composes the same two views: the rendered read view
//! (above) plus the raw source via the generic content mode. [`info`]
//! produces the per-format Info summary (counts / date range / version).
//!
//! Parsing is dependency-free on purpose — the grammar is small and the
//! rendering needs are specific, so a crate's data model would be more to
//! translate than to hand-roll, and peek keeps its lean-runtime stance.

pub mod calendar;
pub mod compose;
pub mod contact;
pub mod datetime;
pub mod info;
mod line;
mod render;

#[cfg(test)]
mod tests;

pub use info::VObjectInfo;

/// Format enum, re-exported from `peek_detect` at the module root so
/// reader code keeps a local `crate::types::vobject::VObjectFormat` path.
pub use peek_detect::types::vobject::VObjectFormat;
