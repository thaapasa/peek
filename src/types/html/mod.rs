//! HTML support: dual-view file like SVG.
//!
//! `RenderedMode` (default) renders the document via `html2text` —
//! lynx-style flow with paragraph wrap, list bullets, table grid,
//! numbered link references, and ANSI styling for bold/italic/colour
//! tags. `ContentMode` (paired) shows the raw HTML source with XML
//! syntax highlighting.
//!
//! [`render`] holds the shared html2text driver — used here and by
//! the EPUB read mode (which renders one chapter at a time).
//!
//! Info gathering currently piggybacks on the structured XML stats
//! path; HTML-specific extras (title, meta, headings outline) can be
//! layered on top later.

pub mod mode;
pub mod render;

pub(crate) use mode::RenderedMode;
