use syntect::highlighting::{Color, Theme};
use syntect::parsing::Scope;

use super::ColorMode;

#[rustfmt::skip] const WHITE: Color = Color { r: 255, g: 255, b: 255, a: 255 };
#[rustfmt::skip] const BLACK: Color = Color { r: 0, g: 0, b: 0, a: 255 };
#[rustfmt::skip] const RED: Color = Color { r: 255, g: 80, b: 80, a: 255 };
#[rustfmt::skip] const YELLOW: Color = Color { r: 255, g: 255, b: 0, a: 255 };

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
            heading: theme.settings.accent.or(keyword_color).unwrap_or(fg),
            label: scope_color(theme, "entity.name").unwrap_or(fg),
            value: scope_color(theme, "string").unwrap_or(fg),
            accent: theme.settings.accent.or(keyword_color).unwrap_or(fg),
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
        let mut out = String::with_capacity(text.len() + 16);
        self.paint_into(&mut out, text, color);
        out
    }

    /// Append `text` to `buf` wrapped in a foreground-color escape and
    /// trailing reset. Avoids the intermediate `String` allocation that
    /// `paint` produces — useful inside hot rendering loops.
    pub fn paint_into(&self, buf: &mut String, text: &str, color: Color) {
        self.color_mode.write_fg_seq(buf, color);
        buf.push_str(text);
        buf.push_str(self.color_mode.reset());
    }

    /// Push a bare foreground-color escape (no text, no reset). Pair with
    /// `push_reset` when emitting `Display`-formatted content directly into
    /// a buffer via `write!`.
    pub fn push_fg(&self, buf: &mut String, color: Color) {
        self.color_mode.write_fg_seq(buf, color);
    }

    /// Push a bare reset escape. See `push_fg`.
    pub fn push_reset(&self, buf: &mut String) {
        buf.push_str(self.color_mode.reset());
    }

    /// Wrap text in a foreground-color escape **without** a trailing reset.
    /// Use this when composing multiple colored segments inside a shared
    /// background (e.g. status lines).
    pub fn paint_fg(&self, text: &str, color: Color) -> String {
        format!("{}{}", self.color_mode.fg_seq(color), text)
    }

    /// Wrap content in a background-color escape with a trailing reset.
    pub fn paint_bg(&self, content: &str, color: Color) -> String {
        format!(
            "{}{}{}",
            self.color_mode.bg_seq(color),
            content,
            self.color_mode.reset()
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
