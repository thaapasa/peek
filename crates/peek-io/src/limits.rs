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
