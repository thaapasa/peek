//! Object-file support (ELF / Mach-O / PE / COFF).
//!
//! Read-only introspection via the `object` crate — one unified API
//! across all four container formats, so there is no per-format parser
//! here. `compose` builds a metadata Info view as the landing page plus
//! two streamed text tables (Sections, Symbols). There is no extract
//! path: sections and symbols are not standalone files.

pub mod compose;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod load;
pub mod tables;
