use std::fmt;
use std::io::Cursor;

use syntect::highlighting::{Theme, ThemeSet};

// ---------------------------------------------------------------------------
// Embedded theme data
// ---------------------------------------------------------------------------

const THEME_IDEA_DARK: &str = include_str!("../../themes/idea-dark.tmTheme");
const THEME_ISLANDS_DARK: &str = include_str!("../../themes/islands-dark.tmTheme");
const THEME_DARK_2026: &str = include_str!("../../themes/dark-2026.tmTheme");
const THEME_VIVID_DARK: &str = include_str!("../../themes/vivid-dark.tmTheme");

/// Supported built-in themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeekThemeName {
    #[default]
    IdeaDark,
    IslandsDark,
    Dark2026,
    VividDark,
}

impl PeekThemeName {
    /// Short CLI name for this theme.
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::IdeaDark => "idea-dark",
            Self::IslandsDark => "islands-dark",
            Self::Dark2026 => "dark-2026",
            Self::VividDark => "vivid-dark",
        }
    }

    /// Embedded .tmTheme source for this theme.
    pub fn tmtheme_source(self) -> &'static str {
        match self {
            Self::IdeaDark => THEME_IDEA_DARK,
            Self::IslandsDark => THEME_ISLANDS_DARK,
            Self::Dark2026 => THEME_DARK_2026,
            Self::VividDark => THEME_VIVID_DARK,
        }
    }

    /// Cycle to the next theme.
    pub fn next(self) -> Self {
        match self {
            Self::IdeaDark => Self::IslandsDark,
            Self::IslandsDark => Self::Dark2026,
            Self::Dark2026 => Self::VividDark,
            Self::VividDark => Self::IdeaDark,
        }
    }

    pub fn help_text(self) -> &'static str {
        match self {
            Self::IdeaDark => "JetBrains IDEA default Dark theme",
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
            Self::IdeaDark,
            Self::IslandsDark,
            Self::Dark2026,
            Self::VividDark,
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(clap::builder::PossibleValue::new(self.cli_name()).help(self.help_text()))
    }
}

/// Parse an embedded .tmTheme string into a syntect Theme.
pub fn load_embedded_theme(source: &str) -> Theme {
    let mut cursor = Cursor::new(source.as_bytes());
    ThemeSet::load_from_reader(&mut cursor).expect("failed to parse embedded theme")
}
