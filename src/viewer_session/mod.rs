//! The interactive viewer session — the bin-side top of the viewer.
//!
//! `ViewerState` (the recursive-peek mode stack + scroll/view cache +
//! extract/descend dispatch) and the `interactive` event loop live here,
//! not in the `viewer` toolkit. They are session-orchestration glue: they
//! reach the `compose` registry, the `gather` hub, and `extract` — all
//! bin-level concerns — so they sit above the foundation/`peek-types`
//! boundary alongside `compose` and `gather`.
//!
//! `ViewerState` is one type across four files, split by concern:
//! `state` (the struct + key dispatch + `apply` + mode switching),
//! `frame` (`SessionFrame` + the stack / descend / extract operations),
//! `prompt` (the modal-prompt slot and its confirm dispatch), and
//! `render` (view cache + failure recovery + scroll math + draw).

pub(crate) mod frame;
pub(crate) mod interactive;
pub(crate) mod prompt;
pub(crate) mod render;
pub(crate) mod state;

#[cfg(test)]
mod state_tests;

use anyhow::Result;
use peek_detect::{ArchiveFormat, CompressionFormat, Detected, FileType};
use peek_foundation::viewer::append_universal_modes;
use peek_foundation::viewer::modes::Mode;
use peek_io::InputSource;
pub(crate) use state::{ModeBuilder, ViewerState};

/// Session-wide access tier. A fresh interactive session starts
/// [`Default`](Access::Default); the first guarded op (a big transparent
/// decompress) lands on Info with a load prompt instead of running up
/// front. Confirming flips the session to [`Unlocked`](Access::Unlocked)
/// for good — later guarded ops proceed without re-asking. `--yes` and
/// the non-interactive paths (which can't answer a prompt) start
/// `Unlocked`. The leaf memory caps do **not** move with this tier; it
/// gates the latency confirmation only (see
/// `docs/large-file-safeguards-plan.md`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Access {
    Default,
    Unlocked,
}

/// Compressed size past which a transparent single-stream decompress is
/// deferred behind a confirmation rather than run eagerly. Decompression
/// is RAM-bounded (it spills past the spool threshold), so this guards
/// *latency*, not memory — a multi-hundred-MB `.xz` can take seconds to
/// expand, and we'd rather land on Info and let the user opt in.
pub(crate) const LATENCY_PROMPT_BYTES: u64 = 50 * 1024 * 1024;

/// Declared-size threshold past which extracting / descending into an
/// entry asks for confirmation first (Default session). Spooling a few
/// hundred MB is quick, so the prompt stays out of the way until an
/// extract is genuinely large; the hard
/// [`MAX_SPILL_BYTES`](peek_io::compression::MAX_SPILL_BYTES) ceiling is
/// the always-on backstop beneath it.
pub(crate) const EXTRACT_PROMPT_BYTES: u64 = 256 * 1024 * 1024;

/// A guarded open held back behind the load prompt — the work the
/// session lands on Info and waits to run until the user confirms.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Deferred {
    /// A big transparent single-stream decompress (`.gz` / `.xz` / …).
    /// Loading runs `resolve_transparent` and reseeds to the inner content.
    Decompress(CompressionFormat),
    /// A big compressed-tar / cpio TOC build — listing it streams the whole
    /// decompressed archive. Loading runs the real compose (the walk) and
    /// reseeds to the listing.
    Listing(ArchiveFormat),
}

/// Decide whether to defer the expensive part of opening `source`. Fires
/// only in a [`Default`](Access::Default) session, and only when the
/// source is big enough ([`LATENCY_PROMPT_BYTES`] of *compressed* bytes)
/// that the op is worth a confirmation: a transparent-decompress wrapper,
/// or a compressed-stream archive whose TOC walk inflates the whole
/// archive. `None` = open eagerly (small, cheap, or already unlocked).
/// Shared by the top-level open (`main::run_view`) and the descend path
/// (`push_extracted`).
pub(crate) fn deferred_open(
    source: &InputSource,
    detected: &Detected,
    access: Access,
) -> Option<Deferred> {
    if access == Access::Unlocked {
        return None;
    }
    let big = matches!(source.byte_len(), Ok(n) if n > LATENCY_PROMPT_BYTES);
    if !big {
        return None;
    }
    match detected.file_type {
        FileType::Compressed(fmt) => Some(Deferred::Decompress(fmt)),
        FileType::Archive(fmt) if fmt.streams_compressed() => Some(Deferred::Listing(fmt)),
        _ => None,
    }
}

/// Build a frame's mode stack, substituting a cheap Hex + Info placeholder
/// when the real compose would run deferred work that hasn't been
/// confirmed yet. A [`Deferred::Decompress`] wrapper already composes to
/// the universal tail (its `file_type` is `Compressed`), so only
/// [`Deferred::Listing`] needs the substitution — its real compose is the
/// expensive TOC walk. `real` runs for every other case.
pub(crate) fn compose_or_defer(
    deferred: Option<Deferred>,
    source: &InputSource,
    real: impl FnOnce() -> Result<Vec<Box<dyn Mode>>>,
) -> Result<Vec<Box<dyn Mode>>> {
    if matches!(deferred, Some(Deferred::Listing(_))) {
        let mut modes = Vec::new();
        append_universal_modes(&mut modes, Some(source))?;
        Ok(modes)
    } else {
        real()
    }
}
