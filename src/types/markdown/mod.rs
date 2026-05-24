//! Markdown viewer + info sidecar.
//!
//! Two views compose for `.md`:
//! 1. Rendered text — `pulldown-cmark` event stream → width-wrapped,
//!    ANSI-styled lines through the shared `RenderedTextMode<R>`.
//! 2. Source — syntax-highlighted markdown via `ContentMode` (same path
//!    every text type uses).
//!
//! The info sidecar (heading/list/code-block/link/image/table counts,
//! reading-time estimate) lives in `info` + `info_gather` + `info_render`
//! and renders into the standard Info section.

pub mod compose;
pub mod info;
pub mod info_gather;
pub mod info_render;
