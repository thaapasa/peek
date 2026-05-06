//! Text engine: streaming UTF-8/UTF-16 stats gathering and the
//! Content/Source info section. Used directly for `FileType::SourceCode`
//! and as a sub-engine by `types::svg` (which augments `TextStats` with
//! SVG-specific extras).

pub mod info_gather;
pub mod info_render;
