use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// Shared syntax highlighting resources.
pub struct ThemeManager {
    pub syntax_set: SyntaxSet,
    pub theme_set: ThemeSet,
    pub theme_name: String,
}

impl ThemeManager {
    pub fn new(theme_name: &str) -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
            theme_name: theme_name.to_string(),
        }
    }

    pub fn theme(&self) -> &syntect::highlighting::Theme {
        self.theme_set
            .themes
            .get(&self.theme_name)
            .unwrap_or_else(|| {
                // Fall back to a known default
                self.theme_set
                    .themes
                    .values()
                    .next()
                    .expect("no themes available")
            })
    }
}
