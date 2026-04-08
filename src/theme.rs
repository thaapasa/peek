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
}

impl PeekTheme {
    /// Derive semantic colors from a syntect theme.
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
        }
    }

    // -- paint helpers -------------------------------------------------------

    /// Wrap text in 24-bit ANSI foreground escape for the given color, with
    /// a trailing reset.
    pub fn paint(&self, text: &str, color: Color) -> String {
        format!("{}{ANSI_RESET}", self.paint_fg(text, color))
    }

    /// Wrap text in 24-bit ANSI foreground escape **without** a trailing reset.
    /// Use this when composing multiple colored segments inside a shared
    /// background (e.g. status lines).
    pub fn paint_fg(&self, text: &str, color: Color) -> String {
        format!("\x1b[38;2;{};{};{}m{}", color.r, color.g, color.b, text)
    }

    /// Wrap content in a 24-bit ANSI background color with a trailing reset.
    pub fn paint_bg(&self, content: &str, color: Color) -> String {
        format!(
            "\x1b[48;2;{};{};{}m{}{ANSI_RESET}",
            color.r, color.g, color.b, content
        )
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

/// Append a foreground-colored character to `buf` (no trailing reset).
/// Intended for per-pixel image rendering in tight loops.
pub fn write_fg(buf: &mut String, r: u8, g: u8, b: u8, ch: char) {
    use std::fmt::Write;
    let _ = write!(buf, "\x1b[38;2;{r};{g};{b}m{ch}");
}

/// Append a character with both foreground and background color to `buf`
/// (no trailing reset).  Intended for block-color image rendering.
pub fn write_fg_bg(buf: &mut String, fg: [u8; 3], bg: [u8; 3], ch: char) {
    use std::fmt::Write;
    let _ = write!(
        buf,
        "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m{}",
        fg[0], fg[1], fg[2], bg[0], bg[1], bg[2], ch
    );
}

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
    peek_theme: PeekTheme,
}

impl ThemeManager {
    pub fn new(theme_name: PeekThemeName) -> Self {
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
            PeekTheme::from_syntect(syntect_theme)
        };
        Self {
            syntax_set,
            theme_set,
            theme_name,
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
}
