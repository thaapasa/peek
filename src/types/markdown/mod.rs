//! Markdown info: counts of headings, code blocks (with declared
//! languages), links, images, tables, lists, task progress, blockquotes,
//! and footnotes — plus a prose-only word count and reading-time
//! estimate. Used as a sidecar to the standard text stats: the source
//! is still rendered as syntax-highlighted Markdown in `ContentMode`.

pub mod info_gather;
pub mod info_render;
