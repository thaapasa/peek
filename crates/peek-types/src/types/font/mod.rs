//! TrueType / OpenType font metadata + per-face rendering. Phase 1
//! surfaces the `name` / `head` / `maxp` / `OS/2` / `cmap` tables as
//! a themed Info section; the source view is omitted because fonts
//! are binary containers (the universal Hex aux mode covers raw byte
//! inspection). Specimen rasterization and per-face listing recursion
//! land in later phases.

pub mod compose;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod sfnt;
pub mod specimen;
pub mod specimen_mode;
pub mod woff;

/// Format enum, re-exported from `peek_detect` at the module root so
/// reader code keeps a local `crate::types::font::FontFormat` path.
pub use peek_detect::types::font::FontFormat;
