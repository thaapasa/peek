//! Foundation-level image-render vocabulary: the config + geometry value
//! types shared by [`PagedImageMode`](crate::viewer::paged::PagedImageMode)
//! and the `types/image` rasterization engine.
//!
//! These moved out of `types/image` so the shared paged mode (and
//! `cell_size`) depend on this module, not the reverse — `types/image`
//! re-exports them back, the allowed reader → foundation direction. The
//! engine (rasterization, glyph atlas, SVG animation) stays in
//! `types/image`; only the config the engine reads and the geometry the
//! viewer drives live here.

mod config;
mod image_mode;
pub mod scroll;
pub mod zoom;
pub mod zoom_pan;

// `pub` (not `pub`) so the `types/image` engine can re-export these
// with `pub use` at the historical `pipeline::*` / `render::*` paths.
pub use config::{Background, FitMode, ImageConfig, TermSize};
pub use image_mode::ImageMode;
pub use scroll::ScrollBounds;
pub use zoom::ZoomLevel;
pub use zoom_pan::{ViewBounds, ZoomPanState};
