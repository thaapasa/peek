//! Apple Finder `.DS_Store` support — the per-folder "Desktop Services
//! Store" (a "Bud1" Buddy-allocator container).
//!
//! Read-only introspection. `compose` builds a summary Info view and a
//! Records table (the shared `viewer::table::TableMode`) listing every
//! stored Finder property — icon positions, window geometry, view style,
//! background — keyed by filename. There is no source view (opaque
//! binary) and no extract path (the records aren't files).

pub mod compose;
pub mod format;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod reader;
pub mod tables;
