//! Jupyter notebook (`.ipynb`) viewer + info sidecar.
//!
//! Two views compose for `.ipynb`:
//! 1. Rendered cells — the notebook is translated to one Markdown
//!    document and rendered through the shared Markdown pipeline
//!    (`RenderedTextMode<NotebookRenderer>`): markdown cells as prose,
//!    code cells as syntax-highlighted fenced blocks, text/stream/error
//!    outputs as fenced output, image outputs as notes. See
//!    [`renderer`] for the translation rationale.
//! 2. Source — the raw notebook JSON via the generic structured content
//!    mode (pretty-printed, `r` toggles raw).
//!
//! The info sidecar (kernel / language, cell + output tallies) lives in
//! `info` + `info_gather` + `info_render`.

pub mod compose;
pub mod info;
pub mod info_gather;
pub mod info_render;
mod model;
mod renderer;

pub use info::NotebookInfo;
pub(crate) use renderer::NotebookRenderer;
