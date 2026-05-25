//! TrueType / OpenType font metadata + per-face rendering. Phase 1
//! surfaces the `name` / `head` / `maxp` / `OS/2` / `cmap` tables as
//! a themed Info section; the source view is omitted because fonts
//! are binary containers (the universal Hex aux mode covers raw byte
//! inspection). Specimen rasterization and per-face listing recursion
//! land in later phases.

pub mod compose;
pub mod detect;
pub mod format;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod specimen;
pub mod specimen_mode;
