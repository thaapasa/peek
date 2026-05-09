//! Low-level ANSI SGR (Select Graphic Rendition) escape mechanics.
//!
//! Pure encoding. No mode awareness, no policy — just functions that
//! emit one specific escape sequence into a buffer or return a static
//! string. The higher layer ([`super::StyleMode`]) decides which form
//! to call based on the active terminal capability budget.
//!
//! Split out from `StyleMode` so attribute toggles, color encoders,
//! and quantization helpers live next to each other and stay
//! independently testable. Adding a new attribute or color encoding
//! touches only this file.

use std::fmt::Write;

/// Reset every SGR attribute (color, bold, italic, …) to the
/// terminal's defaults.
pub const RESET_ALL: &str = "\x1b[0m";

/// Byte form of [`RESET_ALL`] for `write_all` / `Vec<u8>` paths.
pub const RESET_ALL_BYTES: &[u8] = b"\x1b[0m";

/// Reset the foreground color only. Leaves attributes (bold, italic,
/// background) untouched — used when nesting color spans inside an
/// already-styled run.
pub const RESET_FG: &str = "\x1b[39m";

/// Reset the background color only.
pub const RESET_BG: &str = "\x1b[49m";

/// One SGR character attribute. Selectable independently of color so
/// callers (e.g. the HTML renderer) can toggle bold or italic without
/// touching the foreground state.
///
/// Close codes are deliberately specific (`22` / `23` / …) rather than
/// the universal `[0m` so a closing tag inside a styled span only
/// undoes its own attribute.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attr {
    Bold,
    Italic,
    Underline,
    Dim,
    Strikeout,
}

impl Attr {
    /// SGR open sequence for this attribute.
    pub fn open(self) -> &'static str {
        match self {
            Self::Bold => "\x1b[1m",
            Self::Italic => "\x1b[3m",
            Self::Underline => "\x1b[4m",
            Self::Dim => "\x1b[2m",
            Self::Strikeout => "\x1b[9m",
        }
    }

    /// SGR close sequence. Bold and Dim share `[22m` (the canonical
    /// "neither bold nor dim" reset) per ECMA-48 §8.3.117.
    pub fn close(self) -> &'static str {
        match self {
            Self::Bold | Self::Dim => "\x1b[22m",
            Self::Italic => "\x1b[23m",
            Self::Underline => "\x1b[24m",
            Self::Strikeout => "\x1b[29m",
        }
    }
}

// --- color encoders --------------------------------------------------------

pub fn write_fg_truecolor(buf: &mut String, r: u8, g: u8, b: u8) {
    let _ = write!(buf, "\x1b[38;2;{r};{g};{b}m");
}

pub fn write_bg_truecolor(buf: &mut String, r: u8, g: u8, b: u8) {
    let _ = write!(buf, "\x1b[48;2;{r};{g};{b}m");
}

pub fn write_fg_ansi256(buf: &mut String, idx: u8) {
    let _ = write!(buf, "\x1b[38;5;{idx}m");
}

pub fn write_bg_ansi256(buf: &mut String, idx: u8) {
    let _ = write!(buf, "\x1b[48;5;{idx}m");
}

/// Foreground from the 16-color base palette. Indices 0..=7 use the
/// `[3N]` family, 8..=15 use the bright `[9N]` family.
pub fn write_fg_ansi16(buf: &mut String, idx: u8) {
    if idx < 8 {
        let _ = write!(buf, "\x1b[3{idx}m");
    } else {
        let n = idx - 8;
        let _ = write!(buf, "\x1b[9{n}m");
    }
}

/// Background from the 16-color base palette.
pub fn write_bg_ansi16(buf: &mut String, idx: u8) {
    if idx < 8 {
        let _ = write!(buf, "\x1b[4{idx}m");
    } else {
        let n = idx - 8;
        let _ = write!(buf, "\x1b[10{n}m");
    }
}

// --- quantization helpers --------------------------------------------------

/// Rec. 601 luma — common YIQ/YUV approximation. Used to flatten
/// truecolor input to a single luminance channel for grayscale mode.
pub fn rgb_to_luminance(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Quantize one channel into the 6-step xterm cube
/// (0, 95, 135, 175, 215, 255).
fn cube_channel(v: u8) -> u8 {
    if v < 48 {
        0
    } else if v < 115 {
        1
    } else {
        (v - 35) / 40
    }
}

/// Map an RGB triple to the closest entry in the xterm 256-color
/// palette. Greys are routed to the dedicated 24-step grayscale ramp
/// (232..=255) for finer luminance fidelity than the 6×6×6 cube
/// provides.
pub fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
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

/// Map an RGB triple to one of the 16 base ANSI colors. Lossy by
/// design — the base palette is non-uniform and not RGB-aligned.
pub fn rgb_to_ansi16(r: u8, g: u8, b: u8) -> u8 {
    let max = r.max(g).max(b);
    let bright = max > 191;
    let high = if bright { 8 } else { 0 };
    let threshold = if bright { 127 } else { 63 };
    let r_bit = (r as u16 > threshold) as u8;
    let g_bit = (g as u16 > threshold) as u8;
    let b_bit = (b as u16 > threshold) as u8;
    high + (b_bit << 2) + (g_bit << 1) + r_bit
}
