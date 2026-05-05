use std::fmt;

use syntect::highlighting::Color;

/// ANSI escape that resets all attributes.
const ANSI_RESET: &str = "\x1b[0m";

/// Byte form of [`ANSI_RESET`] for use with `write_all`.
const ANSI_RESET_BYTES: &[u8] = b"\x1b[0m";

/// How RGB colors are encoded in the terminal output.
///
/// Callers always paint with truecolor `Color`s; `ColorMode` decides the
/// on-the-wire escape form (or whether to emit one at all). This is the
/// single point of conversion — paint helpers and image writers route
/// through the methods on this enum so the mode can be swapped without
/// touching call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// 24-bit (`\x1b[38;2;r;g;bm`) — full color.
    #[default]
    TrueColor,
    /// 256-color palette (`\x1b[38;5;Nm`).
    Ansi256,
    /// 16-color base palette (`\x1b[3{N}m` / `\x1b[9{N}m`).
    Ansi16,
    /// 24-bit luminance only — preserves shading, drops hue.
    Grayscale,
    /// No escapes — strips all color from the output.
    Plain,
}

impl ColorMode {
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

    /// Foreground SGR sequence for `color`, or `""` in `Plain` mode.
    pub fn fg_seq(self, color: Color) -> String {
        let mut s = String::new();
        self.write_fg_seq(&mut s, color);
        s
    }

    /// Append the foreground SGR sequence for `color` directly to `buf`,
    /// skipping the `String` allocation that `fg_seq` produces.
    pub fn write_fg_seq(self, buf: &mut String, color: Color) {
        use std::fmt::Write;
        match self {
            Self::Plain => {}
            Self::TrueColor => {
                let _ = write!(buf, "\x1b[38;2;{};{};{}m", color.r, color.g, color.b);
            }
            Self::Grayscale => {
                let l = luminance(color.r, color.g, color.b);
                let _ = write!(buf, "\x1b[38;2;{l};{l};{l}m");
            }
            Self::Ansi256 => {
                let _ = write!(buf, "\x1b[38;5;{}m", rgb_to_256(color.r, color.g, color.b));
            }
            Self::Ansi16 => buf.push_str(&fg_ansi16(color.r, color.g, color.b)),
        }
    }

    /// Background SGR sequence for `color`, or `""` in `Plain` mode.
    pub fn bg_seq(self, color: Color) -> String {
        match self {
            Self::Plain => String::new(),
            Self::TrueColor => format!("\x1b[48;2;{};{};{}m", color.r, color.g, color.b),
            Self::Grayscale => {
                let l = luminance(color.r, color.g, color.b);
                format!("\x1b[48;2;{l};{l};{l}m")
            }
            Self::Ansi256 => format!("\x1b[48;5;{}m", rgb_to_256(color.r, color.g, color.b)),
            Self::Ansi16 => bg_ansi16(color.r, color.g, color.b),
        }
    }

    /// SGR reset, or `""` in `Plain` mode (nothing to reset).
    pub fn reset(self) -> &'static str {
        match self {
            Self::Plain => "",
            _ => ANSI_RESET,
        }
    }

    /// Byte form of [`Self::reset`] for `write_all`.
    pub fn reset_bytes(self) -> &'static [u8] {
        match self {
            Self::Plain => b"",
            _ => ANSI_RESET_BYTES,
        }
    }

    /// Append a foreground-colored character to `buf` (no trailing reset).
    /// Hot-loop entry point for image rendering.
    pub fn write_fg(self, buf: &mut String, color: [u8; 3], ch: char) {
        use std::fmt::Write;
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
                let _ = write!(buf, "{}{}", self.fg_seq(c), ch);
            }
        }
    }

    /// Append a character with both foreground and background color to `buf`
    /// (no trailing reset). Hot-loop entry point for block-color image
    /// rendering.
    pub fn write_fg_bg(self, buf: &mut String, fg: [u8; 3], bg: [u8; 3], ch: char) {
        use std::fmt::Write;
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
                let _ = write!(buf, "{}{}{}", self.fg_seq(f), self.bg_seq(b), ch);
            }
        }
    }
}

impl fmt::Display for ColorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.cli_name())
    }
}

impl clap::ValueEnum for ColorMode {
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

// -- conversion helpers -----------------------------------------------------

/// Rec. 601 luma — common YIQ/YUV approximation.
fn luminance(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Quantize one channel into the 6-step xterm cube (0,95,135,175,215,255).
fn cube_channel(v: u8) -> u8 {
    if v < 48 {
        0
    } else if v < 115 {
        1
    } else {
        (v - 35) / 40
    }
}

/// Map an RGB triple to the closest entry in the xterm 256-color palette.
/// Greys are routed to the dedicated 24-step grayscale ramp (232..=255)
/// for finer luminance fidelity than the 6×6×6 cube provides.
fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    if r == g && g == b {
        if r < 8 {
            return 16;
        }
        if r > 248 {
            return 231;
        }
        return 232 + ((r as u32 - 8) / 10).min(23) as u8;
    }
    16 + 36 * cube_channel(r) + 6 * cube_channel(g) + cube_channel(b)
}

/// Map an RGB triple to one of the 16 base ANSI colors. Lossy by design —
/// the base palette is non-uniform and not RGB-aligned.
fn ansi16_index(r: u8, g: u8, b: u8) -> u8 {
    let max = r.max(g).max(b);
    let bright = max > 191;
    let high = if bright { 8 } else { 0 };
    let threshold = if bright { 127 } else { 63 };
    let r_bit = (r as u16 > threshold) as u8;
    let g_bit = (g as u16 > threshold) as u8;
    let b_bit = (b as u16 > threshold) as u8;
    high + (b_bit << 2) + (g_bit << 1) + r_bit
}

fn fg_ansi16(r: u8, g: u8, b: u8) -> String {
    let n = ansi16_index(r, g, b);
    if n < 8 {
        format!("\x1b[3{n}m")
    } else {
        format!("\x1b[9{}m", n - 8)
    }
}

fn bg_ansi16(r: u8, g: u8, b: u8) -> String {
    let n = ansi16_index(r, g, b);
    if n < 8 {
        format!("\x1b[4{n}m")
    } else {
        format!("\x1b[10{}m", n - 8)
    }
}
