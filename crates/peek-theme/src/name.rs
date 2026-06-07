use std::fmt;
use std::io::Cursor;

use syntect::highlighting::{Theme, ThemeSet};

// ---------------------------------------------------------------------------
// Embedded theme data
// ---------------------------------------------------------------------------

const THEME_IDEA_DARK: &str = include_str!("../themes/idea-dark.tmTheme");
const THEME_IDEA_LIGHT: &str = include_str!("../themes/idea-light.tmTheme");
const THEME_SOLARIZED_LIGHT: &str = include_str!("../themes/solarized-light.tmTheme");
const THEME_GITHUB_LIGHT: &str = include_str!("../themes/github-light.tmTheme");
const THEME_VSCODE_DARK_MODERN: &str = include_str!("../themes/vscode-dark-modern.tmTheme");
const THEME_VSCODE_DARK_2026: &str = include_str!("../themes/vscode-dark-2026.tmTheme");
const THEME_VSCODE_MONOKAI: &str = include_str!("../themes/vscode-monokai.tmTheme");
const THEME_GRAVEYARD: &str = include_str!("../themes/graveyard.tmTheme");
const THEME_CANDY_FLOSS: &str = include_str!("../themes/candy-floss.tmTheme");
const THEME_VICTORIAN: &str = include_str!("../themes/victorian.tmTheme");

/// Supported built-in themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeekThemeName {
    #[default]
    IdeaDark,
    IdeaLight,
    SolarizedLight,
    GithubLight,
    VscodeDarkModern,
    VscodeDark2026,
    VscodeMonokai,
    Graveyard,
    CandyFloss,
    Victorian,
}

impl PeekThemeName {
    /// Short CLI name for this theme.
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::IdeaDark => "idea-dark",
            Self::IdeaLight => "idea-light",
            Self::SolarizedLight => "solarized-light",
            Self::GithubLight => "github-light",
            Self::VscodeDarkModern => "vscode-dark-modern",
            Self::VscodeDark2026 => "vscode-dark-2026",
            Self::VscodeMonokai => "vscode-monokai",
            Self::Graveyard => "graveyard",
            Self::CandyFloss => "candy-floss",
            Self::Victorian => "victorian",
        }
    }

    /// Embedded .tmTheme source for this theme.
    pub fn tmtheme_source(self) -> &'static str {
        match self {
            Self::IdeaDark => THEME_IDEA_DARK,
            Self::IdeaLight => THEME_IDEA_LIGHT,
            Self::SolarizedLight => THEME_SOLARIZED_LIGHT,
            Self::GithubLight => THEME_GITHUB_LIGHT,
            Self::VscodeDarkModern => THEME_VSCODE_DARK_MODERN,
            Self::VscodeDark2026 => THEME_VSCODE_DARK_2026,
            Self::VscodeMonokai => THEME_VSCODE_MONOKAI,
            Self::Graveyard => THEME_GRAVEYARD,
            Self::CandyFloss => THEME_CANDY_FLOSS,
            Self::Victorian => THEME_VICTORIAN,
        }
    }

    /// Whether this is a light-background theme. Drives default-theme
    /// selection against the detected terminal background.
    pub fn is_light(self) -> bool {
        matches!(
            self,
            Self::IdeaLight | Self::SolarizedLight | Self::GithubLight
        )
    }

    /// The built-in default for a given terminal background — the favored
    /// dark theme on a dark terminal, the light counterpart on a light one.
    pub fn default_for_light_background(is_light: bool) -> Self {
        if is_light {
            Self::IdeaLight
        } else {
            Self::IdeaDark
        }
    }

    /// Cycle to the next theme.
    pub fn next(self) -> Self {
        match self {
            Self::IdeaDark => Self::IdeaLight,
            Self::IdeaLight => Self::SolarizedLight,
            Self::SolarizedLight => Self::GithubLight,
            Self::GithubLight => Self::VscodeDarkModern,
            Self::VscodeDarkModern => Self::VscodeDark2026,
            Self::VscodeDark2026 => Self::VscodeMonokai,
            Self::VscodeMonokai => Self::Graveyard,
            Self::Graveyard => Self::CandyFloss,
            Self::CandyFloss => Self::Victorian,
            Self::Victorian => Self::IdeaDark,
        }
    }

    /// Cycle to the previous theme.
    pub fn prev(self) -> Self {
        match self {
            Self::IdeaDark => Self::Victorian,
            Self::IdeaLight => Self::IdeaDark,
            Self::SolarizedLight => Self::IdeaLight,
            Self::GithubLight => Self::SolarizedLight,
            Self::VscodeDarkModern => Self::GithubLight,
            Self::VscodeDark2026 => Self::VscodeDarkModern,
            Self::VscodeMonokai => Self::VscodeDark2026,
            Self::Graveyard => Self::VscodeMonokai,
            Self::CandyFloss => Self::Graveyard,
            Self::Victorian => Self::CandyFloss,
        }
    }

    pub fn help_text(self) -> &'static str {
        match self {
            Self::IdeaDark => "JetBrains IDEA default Dark theme",
            Self::IdeaLight => "JetBrains IntelliJ Light theme",
            Self::SolarizedLight => "Solarized Light theme",
            Self::GithubLight => "GitHub Light theme",
            Self::VscodeDarkModern => "VS Code Dark Modern theme",
            Self::VscodeDark2026 => "VS Code Dark 2026 theme",
            Self::VscodeMonokai => "VS Code Monokai theme",
            Self::Graveyard => "Graveyard — gothic moonlit night",
            Self::CandyFloss => "Candy Floss — pastel candy on dark plum",
            Self::Victorian => "Victorian — parlour parchment with oxblood",
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
            Self::IdeaLight,
            Self::SolarizedLight,
            Self::GithubLight,
            Self::VscodeDarkModern,
            Self::VscodeDark2026,
            Self::VscodeMonokai,
            Self::Graveyard,
            Self::CandyFloss,
            Self::Victorian,
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
