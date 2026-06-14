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

/// Nominal RGB of one of the 16 base ANSI colors (xterm defaults).
/// Inverse of [`rgb_to_ansi16`] in spirit only — the base palette is
/// terminal-configurable, so this is a representative value for color
/// comparisons, not a guaranteed display color.
pub fn ansi16_to_rgb(idx: u8) -> (u8, u8, u8) {
    const XTERM16: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    XTERM16[(idx & 0x0f) as usize]
}

/// Nominal RGB of an xterm 256-palette index: the 16 base colors, the
/// 6×6×6 cube (16..=231), and the 24-step grayscale ramp (232..=255).
pub fn ansi256_to_rgb(idx: u8) -> (u8, u8, u8) {
    match idx {
        0..=15 => ansi16_to_rgb(idx),
        16..=231 => {
            let i = idx - 16;
            let level = |c: u8| if c == 0 { 0 } else { 55 + 40 * c };
            (level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        232..=255 => {
            let v = 8 + 10 * (idx - 232);
            (v, v, v)
        }
    }
}

// --- escape-sequence scanning ----------------------------------------------

/// One token of a styled string: a run of plain text, or a complete SGR
/// escape sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sgr<'a> {
    Text(&'a str),
    Esc(&'a str),
}

/// Iterator over the [`Sgr`] tokens of a styled string. See [`scan`].
pub struct SgrScan<'a> {
    s: &'a str,
    i: usize,
}

/// Split a styled string into [`Sgr`] tokens. An escape runs from
/// `\x1b` up to and including the first ASCII letter (the SGR final
/// byte); everything else is text. Never yields an empty token.
pub fn scan(s: &str) -> SgrScan<'_> {
    SgrScan { s, i: 0 }
}

/// Display width of a string with possibly embedded SGR escapes — only
/// text tokens contribute their `UnicodeWidthStr` width; escapes count
/// as zero. Used by every consumer that needs to size styled output
/// against a column budget (wrap, tables, bg-filled lines).
pub fn display_width(s: &str) -> usize {
    let mut w = 0;
    for tok in scan(s) {
        if let Sgr::Text(text) = tok {
            w += unicode_width::UnicodeWidthStr::width(text);
        }
    }
    w
}

/// Strip every SGR escape from `s`, leaving only the visible text. The
/// canonical home for ANSI stripping — readers (notebook tracebacks,
/// markdown) and the search overlay all route through here so the CSI
/// skip logic lives in one place beside [`scan`].
pub fn strip_ansi(s: &str) -> String {
    scan(s)
        .filter_map(|t| match t {
            Sgr::Text(text) => Some(text),
            Sgr::Esc(_) => None,
        })
        .collect()
}

impl<'a> Iterator for SgrScan<'a> {
    type Item = Sgr<'a>;

    fn next(&mut self) -> Option<Sgr<'a>> {
        let bytes = self.s.as_bytes();
        if self.i >= bytes.len() {
            return None;
        }
        let start = self.i;
        if bytes[self.i] == 0x1b {
            self.i += 1;
            while self.i < bytes.len() && !bytes[self.i].is_ascii_alphabetic() {
                self.i += 1;
            }
            if self.i < bytes.len() {
                self.i += 1; // include the SGR final byte
            }
            Some(Sgr::Esc(&self.s[start..self.i]))
        } else {
            while self.i < bytes.len() && bytes[self.i] != 0x1b {
                self.i += 1;
            }
            Some(Sgr::Text(&self.s[start..self.i]))
        }
    }
}

// --- escape classification + active-style tracking -------------------------

/// What an SGR escape does to terminal color state — enough for callers
/// that track the active foreground / background across a styled
/// stream. Character attributes (bold, italic, …) are `Other`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SgrKind {
    ResetAll,
    ResetFg,
    ResetBg,
    Fg,
    Bg,
    Other,
}

/// Classify a complete SGR escape by its leading numeric parameter.
/// Covers every foreground / background form (`38`/`48` extended,
/// `30..=37`/`90..=97` and `40..=47`/`100..=107` base) plus the
/// `0`/`39`/`49` resets; an empty parameter list (`\x1b[m`) is a full
/// reset.
pub fn classify(esc: &str) -> SgrKind {
    match lead_param(esc) {
        0 => SgrKind::ResetAll,
        39 => SgrKind::ResetFg,
        49 => SgrKind::ResetBg,
        38 | 30..=37 | 90..=97 => SgrKind::Fg,
        48 | 40..=47 | 100..=107 => SgrKind::Bg,
        _ => SgrKind::Other,
    }
}

/// Leading numeric parameter of an SGR escape (`0` for an empty list).
fn lead_param(esc: &str) -> u32 {
    esc.strip_prefix("\x1b[")
        .unwrap_or(esc)
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0u32, |acc, b| acc * 10 + (b - b'0') as u32)
}

/// Color-escape encoding family. For callers that must emit *derived*
/// escapes (a swap, a contrast nudge) into a stream without breaking
/// its palette: a 256-color line must stay 256-color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorEncoding {
    TrueColor,
    Ansi256,
    Ansi16,
}

/// Classify a fg/bg color escape's encoding by its leading parameter.
/// Meaningful only for escapes [`classify`] reports as `Fg` / `Bg`.
pub fn color_encoding(esc: &str) -> ColorEncoding {
    match lead_param(esc) {
        38 | 48 => {
            // The mode marker is positional (second parameter) — a
            // substring scan would misread a truecolor component of 5.
            let body = esc.strip_prefix("\x1b[").unwrap_or(esc);
            let body = body.strip_suffix('m').unwrap_or(body);
            match body.split(';').nth(1) {
                Some("5") => ColorEncoding::Ansi256,
                _ => ColorEncoding::TrueColor,
            }
        }
        _ => ColorEncoding::Ansi16,
    }
}

/// Nominal RGB of a fg/bg color escape: truecolor (`38;2;r;g;b`),
/// 256-palette (`38;5;n` via the xterm palette), and base-16
/// (`30..=37` / `90..=97` and bg counterparts, via the nominal xterm
/// table). Returns `None` for malformed or non-color escapes.
pub fn escape_to_rgb(esc: &str) -> Option<(u8, u8, u8)> {
    let body = esc.strip_prefix("\x1b[")?.strip_suffix('m')?;
    let mut parts = body.split(';');
    let lead: u32 = parts.next()?.parse().ok()?;
    match lead {
        38 | 48 => match parts.next()? {
            "2" => {
                let r: u8 = parts.next()?.parse().ok()?;
                let g: u8 = parts.next()?.parse().ok()?;
                let b: u8 = parts.next()?.parse().ok()?;
                Some((r, g, b))
            }
            "5" => {
                let idx: u8 = parts.next()?.parse().ok()?;
                Some(ansi256_to_rgb(idx))
            }
            _ => None,
        },
        30..=37 => Some(ansi16_to_rgb((lead - 30) as u8)),
        90..=97 => Some(ansi16_to_rgb((lead - 90 + 8) as u8)),
        40..=47 => Some(ansi16_to_rgb((lead - 40) as u8)),
        100..=107 => Some(ansi16_to_rgb((lead - 100 + 8) as u8)),
        _ => None,
    }
}

/// Append `esc` converted to the other plane (a fg color emitted as a
/// bg escape or vice versa), preserving its encoding: `38;…` ↔ `48;…`
/// for truecolor / 256, `3x` ↔ `4x` and `9x` ↔ `10x` for the base 16.
/// `to` must be `Fg` or `Bg`; an escape already on that plane is
/// appended unchanged.
pub fn write_replaned(buf: &mut String, esc: &str, to: SgrKind) {
    let body = esc.trim_start_matches("\x1b[").trim_end_matches('m');
    let lead = lead_param(esc);
    let rest = body.split_once(';').map(|(_, r)| r);
    let new_lead = match (lead, to) {
        (38, SgrKind::Bg) => 48,
        (48, SgrKind::Fg) => 38,
        (30..=37, SgrKind::Bg) => lead + 10,
        (40..=47, SgrKind::Fg) => lead - 10,
        (90..=97, SgrKind::Bg) => lead + 10,
        (100..=107, SgrKind::Fg) => lead - 10,
        _ => lead, // already on the requested plane
    };
    match rest {
        Some(rest) => {
            let _ = write!(buf, "\x1b[{new_lead};{rest}m");
        }
        None => {
            let _ = write!(buf, "\x1b[{new_lead}m");
        }
    }
}

/// The foreground + background SGR escapes active at a point in a styled
/// string. String transforms (line wrap, horizontal slice, search-match
/// overlay) feed every escape through [`Self::observe`] and use
/// [`Self::write`] to re-establish the style after a cut — so a color,
/// or a match background, doesn't bleed off or vanish at a boundary.
/// Foreground and background are tracked separately so one can't
/// clobber the other.
#[derive(Default)]
pub struct ActiveStyle {
    fg: String,
    bg: String,
}

impl ActiveStyle {
    /// Update from one complete escape sequence.
    pub fn observe(&mut self, esc: &str) {
        match classify(esc) {
            SgrKind::ResetAll => {
                self.fg.clear();
                self.bg.clear();
            }
            SgrKind::ResetFg => self.fg.clear(),
            SgrKind::ResetBg => self.bg.clear(),
            SgrKind::Fg => {
                self.fg.clear();
                self.fg.push_str(esc);
            }
            SgrKind::Bg => {
                self.bg.clear();
                self.bg.push_str(esc);
            }
            SgrKind::Other => {}
        }
    }

    /// True when no foreground or background escape is active.
    pub fn is_empty(&self) -> bool {
        self.fg.is_empty() && self.bg.is_empty()
    }

    /// The active foreground escape, or `""` when none.
    pub fn fg(&self) -> &str {
        &self.fg
    }

    /// The active background escape, or `""` when none.
    pub fn bg(&self) -> &str {
        &self.bg
    }

    /// Append the active foreground then background escapes to `buf`.
    pub fn write(&self, buf: &mut String) {
        buf.push_str(&self.fg);
        buf.push_str(&self.bg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_splits_text_and_escapes() {
        let s = "\x1b[31mred\x1b[0m plain";
        let tokens: Vec<_> = scan(s).collect();
        assert_eq!(
            tokens,
            vec![
                Sgr::Esc("\x1b[31m"),
                Sgr::Text("red"),
                Sgr::Esc("\x1b[0m"),
                Sgr::Text(" plain"),
            ]
        );
    }

    #[test]
    fn scan_handles_adjacent_escapes_and_empty_input() {
        let tokens: Vec<_> = scan("\x1b[1m\x1b[31mx").collect();
        assert_eq!(
            tokens,
            vec![Sgr::Esc("\x1b[1m"), Sgr::Esc("\x1b[31m"), Sgr::Text("x")]
        );
        assert_eq!(scan("").count(), 0);
    }

    #[test]
    fn classify_covers_color_families() {
        assert_eq!(classify("\x1b[0m"), SgrKind::ResetAll);
        assert_eq!(classify("\x1b[m"), SgrKind::ResetAll);
        assert_eq!(classify("\x1b[39m"), SgrKind::ResetFg);
        assert_eq!(classify("\x1b[49m"), SgrKind::ResetBg);
        assert_eq!(classify("\x1b[38;2;1;2;3m"), SgrKind::Fg);
        assert_eq!(classify("\x1b[31m"), SgrKind::Fg);
        assert_eq!(classify("\x1b[91m"), SgrKind::Fg);
        assert_eq!(classify("\x1b[48;5;200m"), SgrKind::Bg);
        // 16-color background — the form `StyleMode::Ansi16` emits.
        assert_eq!(classify("\x1b[41m"), SgrKind::Bg);
        assert_eq!(classify("\x1b[101m"), SgrKind::Bg);
        assert_eq!(classify("\x1b[1m"), SgrKind::Other);
    }

    #[test]
    fn color_encoding_classifies_by_lead_and_mode_marker() {
        assert_eq!(color_encoding("\x1b[38;2;1;2;3m"), ColorEncoding::TrueColor);
        assert_eq!(color_encoding("\x1b[48;2;1;2;3m"), ColorEncoding::TrueColor);
        assert_eq!(color_encoding("\x1b[38;5;100m"), ColorEncoding::Ansi256);
        assert_eq!(color_encoding("\x1b[48;5;100m"), ColorEncoding::Ansi256);
        // Truecolor with a 5-valued component must not read as 256-palette.
        assert_eq!(
            color_encoding("\x1b[38;2;5;10;20m"),
            ColorEncoding::TrueColor
        );
        assert_eq!(color_encoding("\x1b[48;2;0;5;5m"), ColorEncoding::TrueColor);
        assert_eq!(color_encoding("\x1b[31m"), ColorEncoding::Ansi16);
        assert_eq!(color_encoding("\x1b[103m"), ColorEncoding::Ansi16);
    }

    #[test]
    fn escape_to_rgb_parses_every_color_family() {
        assert_eq!(escape_to_rgb("\x1b[38;2;1;2;3m"), Some((1, 2, 3)));
        assert_eq!(escape_to_rgb("\x1b[48;2;9;8;7m"), Some((9, 8, 7)));
        assert_eq!(escape_to_rgb("\x1b[38;5;231m"), Some(ansi256_to_rgb(231)));
        assert_eq!(escape_to_rgb("\x1b[31m"), Some(ansi16_to_rgb(1)));
        assert_eq!(escape_to_rgb("\x1b[97m"), Some(ansi16_to_rgb(15)));
        assert_eq!(escape_to_rgb("\x1b[44m"), Some(ansi16_to_rgb(4)));
        assert_eq!(escape_to_rgb("\x1b[101m"), Some(ansi16_to_rgb(9)));
        // Malformed / non-color escapes.
        assert_eq!(escape_to_rgb("\x1b[1m"), None);
        assert_eq!(escape_to_rgb("\x1b[38;9;1m"), None);
        assert_eq!(escape_to_rgb("\x1b[38;2;1;2m"), None);
        assert_eq!(escape_to_rgb(""), None);
    }

    #[test]
    fn write_replaned_swaps_planes_in_every_encoding() {
        let replaned = |esc: &str, to: SgrKind| {
            let mut s = String::new();
            write_replaned(&mut s, esc, to);
            s
        };
        assert_eq!(
            replaned("\x1b[38;2;1;2;3m", SgrKind::Bg),
            "\x1b[48;2;1;2;3m"
        );
        assert_eq!(replaned("\x1b[48;5;100m", SgrKind::Fg), "\x1b[38;5;100m");
        assert_eq!(replaned("\x1b[31m", SgrKind::Bg), "\x1b[41m");
        assert_eq!(replaned("\x1b[44m", SgrKind::Fg), "\x1b[34m");
        assert_eq!(replaned("\x1b[97m", SgrKind::Bg), "\x1b[107m");
        assert_eq!(replaned("\x1b[103m", SgrKind::Fg), "\x1b[93m");
        // Already on the requested plane → unchanged.
        assert_eq!(replaned("\x1b[31m", SgrKind::Fg), "\x1b[31m");
    }

    #[test]
    fn active_style_tracks_fg_and_bg_independently() {
        let mut a = ActiveStyle::default();
        a.observe("\x1b[31m");
        a.observe("\x1b[48;2;1;2;3m");
        let mut buf = String::new();
        a.write(&mut buf);
        assert_eq!(buf, "\x1b[31m\x1b[48;2;1;2;3m");
        assert_eq!(a.fg(), "\x1b[31m");

        a.observe("\x1b[49m"); // bg reset only
        assert_eq!(a.fg(), "\x1b[31m");
        let mut buf = String::new();
        a.write(&mut buf);
        assert_eq!(buf, "\x1b[31m");

        a.observe("\x1b[0m"); // full reset
        assert!(a.is_empty());
    }
}
