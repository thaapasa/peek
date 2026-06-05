//! The interactive viewer session — the bin-side top of the viewer.
//!
//! `ViewerState` (the recursive-peek mode stack + scroll/view cache +
//! extract/descend dispatch) and the `interactive` event loop live here,
//! not in the `viewer` toolkit. They are session-orchestration glue: they
//! reach the `compose` registry, the `gather` hub, and `extract` — all
//! bin-level concerns — so they sit above the foundation/`peek-types`
//! boundary alongside `compose` and `gather`.

pub(crate) mod interactive;
pub(crate) mod state;

pub(crate) use state::{ModeBuilder, ViewerState};
