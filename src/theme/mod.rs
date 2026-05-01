//! Theming: built-in theme names, output color encoding, semantic color
//! roles, and the shared syntax-highlighting resources.
//!
//! - [`name`]        — `PeekThemeName`, embedded `.tmTheme` data, parser entry point
//! - [`color_mode`]  — `ColorMode` (truecolor / 256 / 16 / grayscale / plain)
//! - [`peek_theme`]  — `PeekTheme` semantic roles + paint helpers
//! - [`manager`]     — `ThemeManager`: syntax set + theme set + active `PeekTheme`

mod color_mode;
mod manager;
mod name;
mod peek_theme;

pub use color_mode::ColorMode;
pub use manager::ThemeManager;
pub use name::{PeekThemeName, load_embedded_theme};
pub use peek_theme::{PeekTheme, lerp_color};
