use std::fmt;

use syntect::highlighting::{Color, Theme, ThemeSet};
use syntect::parsing::{Scope, SyntaxSet};

/// Supported built-in themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeekThemeName {
    #[default]
    Base16OceanDark,
    Base16EightiesDark,
    Base16MochaDark,
    Base16OceanLight,
    InspiredGitHub,
    SolarizedDark,
    SolarizedLight,
}

impl PeekThemeName {
    /// Short CLI name for this theme.
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::Base16OceanDark => "ocean-dark",
            Self::Base16EightiesDark => "eighties",
            Self::Base16MochaDark => "mocha",
            Self::Base16OceanLight => "ocean-light",
            Self::InspiredGitHub => "github",
            Self::SolarizedDark => "solarized-dark",
            Self::SolarizedLight => "solarized-light",
        }
    }

    /// The exact key string used in syntect's `ThemeSet::load_defaults()`.
    pub fn syntect_key(self) -> &'static str {
        match self {
            Self::Base16OceanDark => "base16-ocean.dark",
            Self::Base16EightiesDark => "base16-eighties.dark",
            Self::Base16MochaDark => "base16-mocha.dark",
            Self::Base16OceanLight => "base16-ocean.light",
            Self::InspiredGitHub => "InspiredGitHub",
            Self::SolarizedDark => "Solarized (dark)",
            Self::SolarizedLight => "Solarized (light)",
        }
    }

    pub fn help_text(self) -> &'static str {
        match self {
            Self::Base16OceanDark => "Dark theme with ocean blues (base16)",
            Self::Base16EightiesDark => "Dark theme with warm eighties palette (base16)",
            Self::Base16MochaDark => "Dark theme with mocha tones (base16)",
            Self::Base16OceanLight => "Light theme with ocean blues (base16)",
            Self::InspiredGitHub => "Light GitHub-inspired theme",
            Self::SolarizedDark => "Solarized dark",
            Self::SolarizedLight => "Solarized light",
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
            Self::Base16OceanDark,
            Self::Base16EightiesDark,
            Self::Base16MochaDark,
            Self::Base16OceanLight,
            Self::InspiredGitHub,
            Self::SolarizedDark,
            Self::SolarizedLight,
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

    /// Wrap text in 24-bit ANSI foreground escape for the given color.
    pub fn paint(&self, text: &str, color: Color) -> String {
        format!("\x1b[38;2;{};{};{}m{}\x1b[0m", color.r, color.g, color.b, text)
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
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let peek_theme = {
            let syntect_theme = theme_set
                .themes
                .get(theme_name.syntect_key())
                .or_else(|| theme_set.themes.values().next())
                .expect("no themes available");
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
            .get(self.theme_name.syntect_key())
            .unwrap_or_else(|| {
                self.theme_set
                    .themes
                    .values()
                    .next()
                    .expect("no themes available")
            })
    }

    pub fn peek_theme(&self) -> &PeekTheme {
        &self.peek_theme
    }
}
