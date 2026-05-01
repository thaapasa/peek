use std::fmt;
use std::io::Cursor;

use syntect::highlighting::{Theme, ThemeSet};

// ---------------------------------------------------------------------------
// Embedded theme data
// ---------------------------------------------------------------------------

const THEME_IDEA_DARK: &str = include_str!("../../themes/idea-dark.tmTheme");
const THEME_VSCODE_DARK_MODERN: &str =
    include_str!("../../themes/vscode-dark-modern.tmTheme");
const THEME_VSCODE_DARK_2026: &str =
    include_str!("../../themes/vscode-dark-2026.tmTheme");
const THEME_VSCODE_MONOKAI: &str = include_str!("../../themes/vscode-monokai.tmTheme");

/// Supported built-in themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeekThemeName {
    #[default]
    IdeaDark,
    VscodeDarkModern,
    VscodeDark2026,
    VscodeMonokai,
}

impl PeekThemeName {
    /// Short CLI name for this theme.
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::IdeaDark => "idea-dark",
            Self::VscodeDarkModern => "vscode-dark-modern",
            Self::VscodeDark2026 => "vscode-dark-2026",
            Self::VscodeMonokai => "vscode-monokai",
        }
    }

    /// Embedded .tmTheme source for this theme.
    pub fn tmtheme_source(self) -> &'static str {
        match self {
            Self::IdeaDark => THEME_IDEA_DARK,
            Self::VscodeDarkModern => THEME_VSCODE_DARK_MODERN,
            Self::VscodeDark2026 => THEME_VSCODE_DARK_2026,
            Self::VscodeMonokai => THEME_VSCODE_MONOKAI,
        }
    }

    /// Cycle to the next theme.
    pub fn next(self) -> Self {
        match self {
            Self::IdeaDark => Self::VscodeDarkModern,
            Self::VscodeDarkModern => Self::VscodeDark2026,
            Self::VscodeDark2026 => Self::VscodeMonokai,
            Self::VscodeMonokai => Self::IdeaDark,
        }
    }

    pub fn help_text(self) -> &'static str {
        match self {
            Self::IdeaDark => "JetBrains IDEA default Dark theme",
            Self::VscodeDarkModern => "VS Code Dark Modern theme",
            Self::VscodeDark2026 => "VS Code Dark 2026 theme",
            Self::VscodeMonokai => "VS Code Monokai theme",
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
            Self::VscodeDarkModern,
            Self::VscodeDark2026,
            Self::VscodeMonokai,
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
