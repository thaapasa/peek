//! Theming: built-in theme names, output color encoding, semantic color
//! roles, and the shared syntax-highlighting resources.
//!
//! - [`name`]        — `PeekThemeName`, embedded `.tmTheme` data, parser entry point
//! - [`style_mode`]  — `StyleMode` (truecolor / 256 / 16 / grayscale / plain)
//! - [`peek_theme`]  — `PeekTheme` semantic roles + paint helpers
//! - [`manager`]     — `ThemeManager`: syntax set + theme set + active `PeekTheme`

mod manager;
mod name;
mod peek_theme;
mod style_mode;

pub use manager::ThemeManager;
pub use name::{PeekThemeName, load_embedded_theme};
pub use peek_theme::{PeekTheme, lerp_color};
pub use style_mode::StyleMode;
