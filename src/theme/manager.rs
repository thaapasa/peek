use std::collections::BTreeMap;

use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use super::{ColorMode, PeekTheme, PeekThemeName, load_embedded_theme};

/// Shared syntax highlighting resources.
pub struct ThemeManager {
    pub syntax_set: SyntaxSet,
    pub theme_set: ThemeSet,
    pub theme_name: PeekThemeName,
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
            peek_theme,
        }
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
