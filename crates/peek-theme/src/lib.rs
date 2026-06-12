//! Theming: built-in theme names, output color/attribute encoding,
//! semantic color roles, and the shared syntax-highlighting resources.
//!
//! - [`name`]        — `PeekThemeName`, embedded `.tmTheme` data, parser entry point
//! - [`sgr`]         — low-level SGR escape mechanics (color encoders, [`Attr`])
//! - [`style_mode`]  — `StyleMode`: emission gate + color encoding budget; delegates to `sgr`
//! - [`peek_theme`]  — `PeekTheme` semantic roles + paint helpers
//! - [`manager`]     — `ThemeManager`: syntax set + theme set + active `PeekTheme`

mod manager;
mod name;
mod peek_theme;
mod sgr;
mod style_mode;

pub use manager::ThemeManager;
pub use name::{PeekThemeName, load_embedded_theme};
pub use peek_theme::{PeekTheme, lerp_color, make_peek_theme};
pub use sgr::{
    ActiveStyle, Attr, ColorEncoding, RESET_ALL, Sgr, SgrKind, ansi16_to_rgb, ansi256_to_rgb,
    classify, color_encoding, display_width, escape_to_rgb, rgb_to_luminance, scan,
    write_fg_ansi16, write_fg_ansi256, write_fg_truecolor, write_replaned,
};
pub use style_mode::StyleMode;
