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

pub(crate) use state::{ModeBuilder, ViewerState};

use peek_detect::{CompressionFormat, Detected, FileType};
use peek_io::InputSource;

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

/// Decide whether to defer transparent decompression of `source`. Returns
/// the codec to defer when `access` is [`Default`](Access::Default) and
/// the source is a [`Compressed`](FileType::Compressed) wrapper larger
/// than [`LATENCY_PROMPT_BYTES`]; `None` means resolve eagerly (small,
/// not compressed, or already unlocked). Shared by the top-level open
/// (`main::run_view`) and the descend path (`push_extracted`).
pub(crate) fn deferred_decompress(
    source: &InputSource,
    detected: &Detected,
    access: Access,
) -> Option<CompressionFormat> {
    if access == Access::Unlocked {
        return None;
    }
    let FileType::Compressed(fmt) = detected.file_type else {
        return None;
    };
    match source.byte_len() {
        Ok(n) if n > LATENCY_PROMPT_BYTES => Some(fmt),
        _ => None,
    }
}
