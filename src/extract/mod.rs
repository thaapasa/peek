//! Extracting an inner item from a container source (animation frame,
//! archive entry, ISO entry) as a standalone [`crate::input::InputSource`].
//!
//! See [`extract`] for the top-level dispatch and [`write`] for how an
//! [`Extracted`] result lands on disk or stdout. Per-type extractors
//! live next to each container's existing module
//! (`types/image/extract.rs`, `types/archive/extract.rs`, etc.).

// `extract::extract` is intentional: this submodule holds the
// top-level dispatch entry point that the rest of the crate calls as
// `crate::extract::extract`. The repetition matches the public path.
#[allow(clippy::module_inception)]
mod extract;
pub mod write;

pub(crate) use extract::sanitize_entry_path;
pub use extract::{ExtractError, ExtractOptions, Extracted, extract};
