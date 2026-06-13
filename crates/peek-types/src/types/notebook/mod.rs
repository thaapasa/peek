//! Jupyter notebook (`.ipynb`) viewer + info sidecar.
//!
//! Three views compose for `.ipynb`:
//! 1. Rendered cells — the notebook is translated to one Markdown
//!    document and rendered through the shared Markdown pipeline
//!    (`RenderedTextMode<NotebookRenderer>`): markdown cells as prose,
//!    code cells as syntax-highlighted fenced blocks, text/stream/error
//!    outputs as fenced output, image outputs as notes. See
//!    [`renderer`] for the translation rationale.
//! 2. Source — the raw notebook JSON via the generic structured content
//!    mode (pretty-printed, `r` toggles raw).
//! 3. Blocks (TOC) — a flat `ListingMode` over the code cells and image
//!    outputs (`listing`), each named `code-N.<ext>` / `image-N.<ext>`.
//!    Extract writes the block to disk; descend recurses into peek over
//!    an in-memory copy (`extract`), so code highlights and images draw
//!    with no notebook-specific descend logic.
//!
//! The info sidecar (kernel / language, cell + output tallies) lives in
//! `info` + `info_gather` + `info_render`.

pub mod compose;
pub mod extract;
pub mod info;
pub mod info_gather;
pub mod info_render;
mod listing;
mod model;
mod renderer;

pub use info::NotebookInfo;
pub(crate) use renderer::NotebookRenderer;
