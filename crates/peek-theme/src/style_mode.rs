use std::fmt;

use syntect::highlighting::Color;

use super::sgr::{
    self, Attr, RESET_ALL, RESET_ALL_BYTES, RESET_BG, RESET_FG, rgb_to_ansi16, rgb_to_ansi256,
    rgb_to_luminance,
};

/// Active SGR (color + attribute) emission budget.
///
/// Callers always paint with truecolor `Color`s and full attribute
/// names; `StyleMode` decides the on-the-wire escape form (or whether
/// to emit one at all). The single point of policy — paint helpers,
/// image writers, and HTML rendering all route through these methods
/// so the mode can be swapped without touching call sites.
///
/// Two axes collapsed into one user-facing knob (CLI `--color`):
/// emission gate (`Plain` strips everything) plus color encoding for
/// the non-plain modes. Attributes (bold / italic / …) ride on the
/// emission gate; they're emitted whenever any escape would be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StyleMode {
    /// 24-bit (`\x1b[38;2;r;g;bm`) — full color.
    #[default]
    TrueColor,
    /// 256-color palette (`\x1b[38;5;Nm`).
    Ansi256,
    /// 16-color base palette (`\x1b[3{N}m` / `\x1b[9{N}m`).
    Ansi16,
    /// 24-bit luminance only — preserves shading, drops hue.
    Grayscale,
    /// No escapes — strips all color and attribute escapes.
    Plain,
}

impl StyleMode {
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::TrueColor => "truecolor",
            Self::Ansi256 => "256",
            Self::Ansi16 => "16",
            Self::Grayscale => "grayscale",
            Self::Plain => "plain",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::TrueColor => Self::Ansi256,
            Self::Ansi256 => Self::Ansi16,
            Self::Ansi16 => Self::Grayscale,
            Self::Grayscale => Self::Plain,
            Self::Plain => Self::TrueColor,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::TrueColor => Self::Plain,
            Self::Ansi256 => Self::TrueColor,
            Self::Ansi16 => Self::Ansi256,
            Self::Grayscale => Self::Ansi16,
            Self::Plain => Self::Grayscale,
        }
    }

    pub fn help_text(self) -> &'static str {
        match self {
            Self::TrueColor => "24-bit RGB (default)",
            Self::Ansi256 => "256-color palette",
            Self::Ansi16 => "16-color base palette",
            Self::Grayscale => "luminance only (no hue)",
            Self::Plain => "no color escapes",
        }
    }

    /// True when any escape would be emitted at all. `false` only for
    /// `Plain`. Cheaper to call than building a string when the caller
    /// just needs to gate behavior.
    pub fn styled(self) -> bool {
        !matches!(self, Self::Plain)
    }

    /// Foreground SGR sequence for `color`, or `""` in `Plain` mode.
    pub fn fg_seq(self, color: Color) -> String {
        let mut s = String::new();
        self.write_fg_seq(&mut s, color);
        s
    }

    /// Append the foreground SGR sequence for `color` directly to `buf`,
    /// skipping the `String` allocation that `fg_seq` produces.
    pub fn write_fg_seq(self, buf: &mut String, color: Color) {
        match self {
            Self::Plain => {}
            Self::TrueColor => sgr::write_fg_truecolor(buf, color.r, color.g, color.b),
            Self::Grayscale => {
                let l = rgb_to_luminance(color.r, color.g, color.b);
                sgr::write_fg_truecolor(buf, l, l, l);
            }
            Self::Ansi256 => sgr::write_fg_ansi256(buf, rgb_to_ansi256(color.r, color.g, color.b)),
            Self::Ansi16 => sgr::write_fg_ansi16(buf, rgb_to_ansi16(color.r, color.g, color.b)),
        }
    }

    /// Background SGR sequence for `color`, or `""` in `Plain` mode.
    pub fn bg_seq(self, color: Color) -> String {
        let mut s = String::new();
        self.write_bg_seq(&mut s, color);
        s
    }

    /// Append the background SGR sequence for `color` directly to `buf`.
    pub fn write_bg_seq(self, buf: &mut String, color: Color) {
        match self {
            Self::Plain => {}
            Self::TrueColor => sgr::write_bg_truecolor(buf, color.r, color.g, color.b),
            Self::Grayscale => {
                let l = rgb_to_luminance(color.r, color.g, color.b);
                sgr::write_bg_truecolor(buf, l, l, l);
            }
            Self::Ansi256 => sgr::write_bg_ansi256(buf, rgb_to_ansi256(color.r, color.g, color.b)),
            Self::Ansi16 => sgr::write_bg_ansi16(buf, rgb_to_ansi16(color.r, color.g, color.b)),
        }
    }

    /// Open sequence for `attr`, or `""` in `Plain` mode. Pair with
    /// [`Self::attr_close`] to bracket a styled span.
    pub fn attr_open(self, attr: Attr) -> &'static str {
        if self.styled() { attr.open() } else { "" }
    }

    /// Close sequence for `attr`. Specific (e.g. `[22m` for bold)
    /// rather than universal `[0m`, so closing one attribute inside a
    /// nested span doesn't blow away the outer state.
    pub fn attr_close(self, attr: Attr) -> &'static str {
        if self.styled() { attr.close() } else { "" }
    }

    /// Universal SGR reset, or `""` in `Plain` mode.
    pub fn reset(self) -> &'static str {
        if self.styled() { RESET_ALL } else { "" }
    }

    /// Byte form of [`Self::reset`] for `write_all`.
    pub fn reset_bytes(self) -> &'static [u8] {
        if self.styled() { RESET_ALL_BYTES } else { b"" }
    }

    /// Reset only the foreground color (preserve attributes / bg).
    pub fn reset_fg(self) -> &'static str {
        if self.styled() { RESET_FG } else { "" }
    }

    /// Reset only the background color.
    pub fn reset_bg(self) -> &'static str {
        if self.styled() { RESET_BG } else { "" }
    }

    /// Append a foreground-colored character to `buf` (no trailing reset).
    /// Hot-loop entry point for image rendering.
    pub fn write_fg(self, buf: &mut String, color: [u8; 3], ch: char) {
        let c = Color {
            r: color[0],
            g: color[1],
            b: color[2],
            a: 255,
        };
        match self {
            Self::Plain => {
                buf.push(ch);
            }
            _ => {
                self.write_fg_seq(buf, c);
                buf.push(ch);
            }
        }
    }

    /// Append a character with both foreground and background color to `buf`
    /// (no trailing reset). Hot-loop entry point for block-color image
    /// rendering.
    pub fn write_fg_bg(self, buf: &mut String, fg: [u8; 3], bg: [u8; 3], ch: char) {
        let f = Color {
            r: fg[0],
            g: fg[1],
            b: fg[2],
            a: 255,
        };
        let b = Color {
            r: bg[0],
            g: bg[1],
            b: bg[2],
            a: 255,
        };
        match self {
            Self::Plain => {
                buf.push(ch);
            }
            _ => {
                self.write_fg_seq(buf, f);
                self.write_bg_seq(buf, b);
                buf.push(ch);
            }
        }
    }
}

impl fmt::Display for StyleMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.cli_name())
    }
}

impl clap::ValueEnum for StyleMode {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self::TrueColor,
            Self::Ansi256,
            Self::Ansi16,
            Self::Grayscale,
            Self::Plain,
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(clap::builder::PossibleValue::new(self.cli_name()).help(self.help_text()))
    }
}
