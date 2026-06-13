//! Memory-budget classes — the one place the byte-cap *sizes* are
//! decided. Every size gate in the workspace aliases one of these three
//! classes; the per-site constants keep their domain names and local
//! rationale, this module owns the numbers so they can be surveyed and
//! tuned coherently. (See `docs/architecture.md` → "Memory budgets" for
//! the full member table and the gate helpers to call.)
//!
//! The classes are named by *consumption shape*, not by feature — that's
//! what makes them layer-neutral enough to live in the bottom crate:
//!
//! - [`WHOLE_DOC_BYTES`]: the input is materialized **and transformed**
//!   into a larger in-memory form (parse tree, styled lines). Expansion
//!   runs 5–20×, and the transform blocks the single-threaded UI, so
//!   this is the smallest class. 32 MB ≈ ~300–600 MB peak and ~1–2 s of
//!   parse+highlight worst case.
//! - [`SIDECAR_PARSE_BYTES`]: the input is materialized whole but the
//!   derived output is small (text stats, a plist's fields). Same read
//!   cost as whole-doc, no expansion — so it tolerates more.
//! - [`BULK_WALK_BYTES`]: one bounded pass over an untrusted or
//!   unbounded stream (decompression output, archive entry, search
//!   scan). Nothing proportional is retained; the cap bounds the walk
//!   itself.
//!
//! Membership is by rationale, not by number — a cap that happens to
//! share a size but guards a different shape (per-record caps, pixel
//! ceilings, count caps) stays local to its site.

/// Materialize-and-transform budget: whole-document renders (HTML /
/// DOCX / RTF / Markdown / notebook), structured pretty-print, zip-entry
/// payloads feeding those renders, the UTF-16 CSV transcode.
pub const WHOLE_DOC_BYTES: u64 = 32 * 1024 * 1024;

/// Whole-text parse with small derived output: sidecar text parsers
/// (markdown / SQL / CSS info), the UTF-16 text-stats decode, DMG
/// property-list extraction.
pub const SIDECAR_PARSE_BYTES: u64 = 64 * 1024 * 1024;

/// Single bounded walk over untrusted / unbounded data: the batch
/// decompress helper (`decompress_bytes`; the streaming transparent path
/// spills to a tempfile and is bounded by its spool threshold instead),
/// per-entry archive extraction, the raw-content search
/// scan, the static-library object-member summary (materialized whole,
/// but held for one pass with no expansion — and real `.a` files run
/// hundreds of MB, past the sidecar budget).
pub const BULK_WALK_BYTES: u64 = 256 * 1024 * 1024;

/// The budget a whole-file read must name. Every
/// [`InputSource::read_bytes`](crate::InputSource::read_bytes) /
/// [`read_text`](crate::InputSource::read_text) call passes one, so an
/// unguarded slurp is not expressible — the only escape is the loud,
/// greppable [`Unbounded`](Budget::Unbounded). The three capped variants
/// alias the classes above by *consumption shape*; the carried `&str` is
/// the `what`-name in the over-cap error message.
///
/// This is the carrier the later session-unlock work threads a tier
/// through ([`cap`](Budget::cap) will resolve per `Access`); today it maps
/// to the fixed Default class constant.
#[derive(Debug, Clone, Copy)]
pub enum Budget {
    /// Materialize **and transform** into a larger form (parse tree,
    /// styled lines, decoded image). Caps at [`WHOLE_DOC_BYTES`].
    WholeDoc(&'static str),
    /// Materialize whole, derive something small (stats, header fields).
    /// Caps at [`SIDECAR_PARSE_BYTES`].
    Sidecar(&'static str),
    /// One bounded pass over untrusted / unbounded data, nothing
    /// proportional retained. Caps at [`BULK_WALK_BYTES`].
    BulkWalk(&'static str),
    /// Bounded by construction — the explicit, greppable escape hatch.
    /// The `&str` names *why* the read is safe (a small extracted entry,
    /// an in-memory frame, a structurally tiny format): the one-line
    /// comment the old review rule asked for, now a required argument.
    Unbounded(&'static str),
}

impl Budget {
    /// The byte cap to enforce before reading, or `None` for
    /// [`Unbounded`](Budget::Unbounded) (read straight, no stat).
    pub const fn cap(&self) -> Option<u64> {
        match self {
            Self::WholeDoc(_) => Some(WHOLE_DOC_BYTES),
            Self::Sidecar(_) => Some(SIDECAR_PARSE_BYTES),
            Self::BulkWalk(_) => Some(BULK_WALK_BYTES),
            Self::Unbounded(_) => None,
        }
    }

    /// The carried `what` / why-safe label.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::WholeDoc(w) | Self::Sidecar(w) | Self::BulkWalk(w) | Self::Unbounded(w) => w,
        }
    }
}
