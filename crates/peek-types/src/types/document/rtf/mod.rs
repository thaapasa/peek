//! RTF support: control-word markup, single file (no container).
//!
//! One view: [`renderer::RtfRenderer`] renders the styled body. There
//! is no listing or extract — RTF isn't a container.

pub mod extract;
pub mod info_gather;
pub mod parse;
pub mod render;
pub mod renderer;

pub(crate) use renderer::RtfRenderer;
