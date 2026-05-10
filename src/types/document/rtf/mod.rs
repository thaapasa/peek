//! RTF support: control-word markup, single file (no container).
//!
//! One view: [`read_mode::RtfReadMode`] renders the styled body. There
//! is no listing or extract — RTF isn't a container.

pub mod extract;
pub mod info_gather;
pub mod parse;
pub mod read_mode;
pub mod render;

pub(crate) use read_mode::RtfReadMode;
