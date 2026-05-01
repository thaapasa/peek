use std::collections::BTreeMap;
use std::fmt;
use std::io::Cursor;

use syntect::highlighting::{Color, Theme, ThemeSet};
use syntect::parsing::{Scope, SyntaxSet};

// ---------------------------------------------------------------------------
// Embedded theme data
// ---------------------------------------------------------------------------

const THEME_ISLANDS_DARK: &str = include_str!("../themes/islands-dark.tmTheme");
const THEME_DARK_2026: &str = include_str!("../themes/dark-2026.tmTheme");
const THEME_VIVID_DARK: &str = include_str!("../themes/vivid-dark.tmTheme");

/// Supported built-in themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeekThemeName {
    #[default]
    IslandsDark,
    Dark2026,
    VividDark,
}

impl PeekThemeName {
    /// Short CLI name for this theme.
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::IslandsDark => "islands-dark",
            Self::Dark2026 => "dark-2026",
            Self::VividDark => "vivid-dark",
        }
    }

    /// Embedded .tmTheme source for this theme.
    pub fn tmtheme_source(self) -> &'static str {
        match self {
            Self::IslandsDark => THEME_ISLANDS_DARK,
            Self::Dark2026 => THEME_DARK_2026,
            Self::VividDark => THEME_VIVID_DARK,
        }
    }

    /// Cycle to the next theme.
    pub fn next(self) -> Self {
        match self {
            Self::IslandsDark => Self::Dark2026,
            Self::Dark2026 => Self::VividDark,
            Self::VividDark => Self::IslandsDark,
        }
    }

    pub fn help_text(self) -> &'static str {
        match self {
            Self::IslandsDark => "JetBrains Islands-inspired dark theme",
            Self::Dark2026 => "VS Code Dark 2026-inspired theme",
            Self::VividDark => "High-contrast dark theme with vivid colors",
        }
    }
}

impl fmt::Display for PeekThemeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.cli_name())
    }
}

impl clap::ValueEnum for PeekThemeName {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self::IslandsDark,
            Self::Dark2026,
            Self::VividDark,
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(clap::builder::PossibleValue::new(self.cli_name()).help(self.help_text()))
    }
}

// ---------------------------------------------------------------------------
// Color output mode
// ---------------------------------------------------------------------------

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
        match self {
            Self::Plain => String::new(),
            Self::TrueColor => format!("\x1b[38;2;{};{};{}m", color.r, color.g, color.b),
            Self::Grayscale => {
                let l = luminance(color.r, color.g, color.b);
                format!("\x1b[38;2;{l};{l};{l}m")
            }
            Self::Ansi256 => format!("\x1b[38;5;{}m", rgb_to_256(color.r, color.g, color.b)),
            Self::Ansi16 => fg_ansi16(color.r, color.g, color.b),
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
        let c = Color { r: color[0], g: color[1], b: color[2], a: 255 };
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
        let f = Color { r: fg[0], g: fg[1], b: fg[2], a: 255 };
        let b = Color { r: bg[0], g: bg[1], b: bg[2], a: 255 };
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
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round().clamp(0.0, 255.0) as u8
}

/// Quantize one channel into the 6-step xterm cube (0,95,135,175,215,255).
fn cube_channel(v: u8) -> u8 {
    if v < 48 { 0 }
    else if v < 115 { 1 }
    else { (v - 35) / 40 }
}

/// Map an RGB triple to the closest entry in the xterm 256-color palette.
/// Greys are routed to the dedicated 24-step grayscale ramp (232..=255)
/// for finer luminance fidelity than the 6×6×6 cube provides.
fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    if r == g && g == b {
        if r < 8 { return 16; }
        if r > 248 { return 231; }
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

// ---------------------------------------------------------------------------
// Semantic color roles
// ---------------------------------------------------------------------------

const WHITE: Color = Color {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};
const BLACK: Color = Color {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};
const RED: Color = Color {
    r: 255,
    g: 80,
    b: 80,
    a: 255,
};
const YELLOW: Color = Color {
    r: 255,
    g: 255,
    b: 0,
    a: 255,
};

/// Semantic color roles for all non-syntax UI output.
#[derive(Clone)]
#[allow(unused)]
pub struct PeekTheme {
    pub foreground: Color,
    pub background: Color,
    pub heading: Color,
    pub label: Color,
    pub value: Color,
    pub accent: Color,
    pub muted: Color,
    pub warning: Color,
    pub gutter: Color,
    pub search_match: Color,
    pub selection: Color,
    /// Output color encoding. Toggled at runtime — paint helpers read
    /// this on each call so a cycle invalidating the line cache is enough
    /// to switch the whole UI.
    pub color_mode: ColorMode,
}

impl PeekTheme {
    /// Derive semantic colors from a syntect theme. `color_mode` defaults
    /// to `TrueColor`; callers override it after construction.
    pub fn from_syntect(theme: &Theme) -> Self {
        let fg = theme.settings.foreground.unwrap_or(WHITE);
        let bg = theme.settings.background.unwrap_or(BLACK);

        let keyword_color = scope_color(theme, "keyword");
        let muted = scope_color(theme, "comment").unwrap_or_else(|| blend(fg, bg, 0.5));

        Self {
            foreground: fg,
            background: bg,
            heading: theme
                .settings
                .accent
                .or(keyword_color)
                .unwrap_or(fg),
            label: scope_color(theme, "entity.name").unwrap_or(fg),
            value: scope_color(theme, "string").unwrap_or(fg),
            accent: theme
                .settings
                .accent
                .or(keyword_color)
                .unwrap_or(fg),
            muted,
            warning: scope_color(theme, "invalid").unwrap_or(RED),
            gutter: theme.settings.gutter_foreground.unwrap_or(muted),
            search_match: theme.settings.find_highlight.unwrap_or(YELLOW),
            selection: theme
                .settings
                .selection
                .unwrap_or_else(|| blend(bg, fg, 0.15)),
            color_mode: ColorMode::TrueColor,
        }
    }

    // -- paint helpers -------------------------------------------------------

    /// Wrap text in a foreground-color escape with a trailing reset.
    pub fn paint(&self, text: &str, color: Color) -> String {
        format!("{}{}{}", self.color_mode.fg_seq(color), text, self.color_mode.reset())
    }

    /// Wrap text in a foreground-color escape **without** a trailing reset.
    /// Use this when composing multiple colored segments inside a shared
    /// background (e.g. status lines).
    pub fn paint_fg(&self, text: &str, color: Color) -> String {
        format!("{}{}", self.color_mode.fg_seq(color), text)
    }

    /// Wrap content in a background-color escape with a trailing reset.
    pub fn paint_bg(&self, content: &str, color: Color) -> String {
        format!("{}{}{}", self.color_mode.bg_seq(color), content, self.color_mode.reset())
    }

    pub fn paint_heading(&self, text: &str) -> String {
        self.paint(text, self.heading)
    }

    pub fn paint_label(&self, text: &str) -> String {
        self.paint(text, self.label)
    }

    pub fn paint_value(&self, text: &str) -> String {
        self.paint(text, self.value)
    }

    pub fn paint_accent(&self, text: &str) -> String {
        self.paint(text, self.accent)
    }

    pub fn paint_muted(&self, text: &str) -> String {
        self.paint(text, self.muted)
    }

    #[allow(unused)]
    pub fn paint_warning(&self, text: &str) -> String {
        self.paint(text, self.warning)
    }
}

// -- ANSI constants & free helpers ------------------------------------------

/// ANSI escape that resets all attributes.
pub const ANSI_RESET: &str = "\x1b[0m";

/// Byte form of [`ANSI_RESET`] for use with `write_all`.
pub const ANSI_RESET_BYTES: &[u8] = b"\x1b[0m";

/// Linearly interpolate between two colors.
pub fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: (a.r as f32 + (b.r as f32 - a.r as f32) * t) as u8,
        g: (a.g as f32 + (b.g as f32 - a.g as f32) * t) as u8,
        b: (a.b as f32 + (b.b as f32 - a.b as f32) * t) as u8,
        a: 255,
    }
}

/// Blend two colors by factor t (0.0 = all `a`, 1.0 = all `b`).
fn blend(a: Color, b: Color, t: f32) -> Color {
    lerp_color(a, b, t)
}

/// Find the foreground color for a scope name in the theme.
fn scope_color(theme: &Theme, scope_name: &str) -> Option<Color> {
    let scope = Scope::new(scope_name).ok()?;
    let stack = [scope];

    let mut best_color = None;
    let mut best_score = None;

    for item in &theme.scopes {
        if let Some(score) = item.scope.does_match(&stack)
            && best_score.is_none_or(|best| score > best)
            && let Some(fg) = item.style.foreground
        {
            best_color = Some(fg);
            best_score = Some(score);
        }
    }

    best_color
}

/// Parse an embedded .tmTheme string into a syntect Theme.
pub fn load_embedded_theme(source: &str) -> Theme {
    let mut cursor = Cursor::new(source.as_bytes());
    ThemeSet::load_from_reader(&mut cursor).expect("failed to parse embedded theme")
}

// ---------------------------------------------------------------------------
// ThemeManager
// ---------------------------------------------------------------------------

/// Shared syntax highlighting resources.
pub struct ThemeManager {
    pub syntax_set: SyntaxSet,
    pub theme_set: ThemeSet,
    pub theme_name: PeekThemeName,
    color_mode: ColorMode,
    peek_theme: PeekTheme,
}

impl ThemeManager {
    pub fn new(theme_name: PeekThemeName, color_mode: ColorMode) -> Self {
        let syntax_set = two_face::syntax::extra_no_newlines();

        // Load all custom themes into a ThemeSet
        let mut themes = BTreeMap::new();
        for variant in <PeekThemeName as clap::ValueEnum>::value_variants() {
            themes.insert(
                variant.cli_name().to_string(),
                load_embedded_theme(variant.tmtheme_source()),
            );
        }
        let theme_set = ThemeSet { themes };

        let peek_theme = {
            let syntect_theme = theme_set
                .themes
                .get(theme_name.cli_name())
                .expect("theme must exist");
            let mut t = PeekTheme::from_syntect(syntect_theme);
            t.color_mode = color_mode;
            t
        };
        Self {
            syntax_set,
            theme_set,
            theme_name,
            color_mode,
            peek_theme,
        }
    }

    pub fn theme(&self) -> &syntect::highlighting::Theme {
        self.theme_set
            .themes
            .get(self.theme_name.cli_name())
            .expect("theme must exist")
    }

    pub fn theme_for(&self, name: PeekThemeName) -> &syntect::highlighting::Theme {
        self.theme_set
            .themes
            .get(name.cli_name())
            .expect("theme must exist")
    }

    pub fn peek_theme(&self) -> &PeekTheme {
        &self.peek_theme
    }

    pub fn color_mode(&self) -> ColorMode {
        self.color_mode
    }
}
