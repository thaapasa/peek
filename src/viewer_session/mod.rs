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
