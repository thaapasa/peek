//! Line-number gutter for `ContentMode`'s text views.
//!
//! The gutter is a right-aligned line-number column plus a ` │ `
//! separator, painted in the theme's gutter colour. Its width is sized
//! from the source's *total* line count — not the visible window — so
//! it stays stable as the viewport scrolls. Continuation rows of a
//! soft-wrapped logical line get a blank gutter of the same width, so
//! wrapped text still lines up under its first row.

use crate::theme::PeekTheme;

/// Visible width of the ` │ ` separator that trails the number column.
const SEPARATOR_WIDTH: usize = 3;

/// The line-number gutter — its on/off state plus the painting logic.
pub(crate) struct Gutter {
    /// Whether the gutter is shown — flipped by the line-numbers key.
    enabled: bool,
}

impl Gutter {
    pub(crate) fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }

    /// Digit count for a gutter sized to `total` lines — minimum 2, so
    /// a 9-line file's gutter doesn't bounce in width past line 9.
    pub(crate) fn digit_width(total: usize) -> usize {
        let mut digits = 1;
        let mut n = total;
        while n >= 10 {
            n /= 10;
            digits += 1;
        }
        digits.max(2)
    }

    /// Visible-cell width of the gutter, separator included. Zero when
    /// disabled or the source is empty.
    pub(crate) fn visible_width(&self, total: usize) -> usize {
        if self.enabled && total > 0 {
            Self::digit_width(total) + SEPARATOR_WIDTH
        } else {
            0
        }
    }

    /// Gutter prefix for one visual row. `line_num` is `Some` for the
    /// first wrap segment of a logical line, `None` for a continuation
    /// row (blank gutter, same width). Empty string when disabled.
    pub(crate) fn prefix(
        &self,
        line_num: Option<usize>,
        total: usize,
        theme: &PeekTheme,
    ) -> String {
        if !self.enabled || total == 0 {
            return String::new();
        }
        let width = Self::digit_width(total);
        let style_mode = theme.style_mode;
        let fg = style_mode.fg_seq(theme.gutter);
        let reset = style_mode.reset();
        match line_num {
            Some(n) => format!("{fg}{n:>width$} │ {reset}"),
            None => format!("{fg}{:>width$} │ {reset}", ""),
        }
    }

    /// Streaming prefix builder for the pipe path. Returns a closure
    /// that takes a 1-based line number and yields the gutter prefix —
    /// `None` when the gutter is disabled or the source is empty, so
    /// callers can skip the concat entirely. Width and paint sequences
    /// are hoisted out of the loop once, matching [`apply`]'s perf
    /// shape but compatible with streamed line iteration where the
    /// line set isn't materialised up front.
    pub(crate) fn stream_prefixer(
        &self,
        total: usize,
        theme: &PeekTheme,
    ) -> impl Fn(usize) -> Option<String> + use<> {
        let enabled = self.enabled && total > 0;
        let width = Self::digit_width(total);
        let style_mode = theme.style_mode;
        let fg = style_mode.fg_seq(theme.gutter);
        let reset = style_mode.reset();
        move |n: usize| -> Option<String> {
            if !enabled {
                return None;
            }
            Some(format!("{fg}{n:>width$} │ {reset}"))
        }
    }

    /// Prepend the gutter to each already-rendered line in place — the
    /// print/pipe path, where every line is its own logical line.
    /// `start` is the 0-based source index of `lines[0]`. No-op when
    /// disabled. Paint sequences are hoisted out of the loop since this
    /// runs over the whole file, not a viewport.
    pub(crate) fn apply(
        &self,
        lines: &mut [String],
        start: usize,
        total: usize,
        theme: &PeekTheme,
    ) {
        if !self.enabled || total == 0 || lines.is_empty() {
            return;
        }
        let width = Self::digit_width(total);
        let style_mode = theme.style_mode;
        let fg_open = style_mode.fg_seq(theme.gutter);
        let reset = style_mode.reset();
        for (offset, line) in lines.iter_mut().enumerate() {
            let n = start + offset + 1;
            let gutter = format!("{fg_open}{n:>width$} │ {reset}");
            let mut prefixed = String::with_capacity(gutter.len() + line.len());
            prefixed.push_str(&gutter);
            prefixed.push_str(line);
            *line = prefixed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{PeekThemeName, StyleMode, ThemeManager};

    fn plain_theme() -> PeekTheme {
        ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain)
            .peek_theme()
            .clone()
    }

    #[test]
    fn digit_width_has_a_floor_of_two() {
        assert_eq!(Gutter::digit_width(0), 2);
        assert_eq!(Gutter::digit_width(9), 2);
        assert_eq!(Gutter::digit_width(99), 2);
        assert_eq!(Gutter::digit_width(100), 3);
        assert_eq!(Gutter::digit_width(12345), 5);
    }

    #[test]
    fn disabled_gutter_takes_no_width_and_emits_nothing() {
        let g = Gutter::new(false);
        let theme = plain_theme();
        assert_eq!(g.visible_width(500), 0);
        assert_eq!(g.prefix(Some(3), 500, &theme), "");
    }

    #[test]
    fn enabled_prefix_right_aligns_the_number() {
        let g = Gutter::new(true);
        let theme = plain_theme(); // plain style → no escape sequences
        assert_eq!(g.visible_width(500), 6); // 3 digits + " │ "
        assert_eq!(g.prefix(Some(7), 500, &theme), "  7 │ ");
        // Continuation row: blank gutter, same width.
        assert_eq!(g.prefix(None, 500, &theme), "    │ ");
    }

    #[test]
    fn apply_prepends_source_indexed_numbers() {
        let g = Gutter::new(true);
        let theme = plain_theme();
        let mut lines = vec!["alpha".to_string(), "beta".to_string()];
        g.apply(&mut lines, 10, 200, &theme);
        assert_eq!(lines[0], " 11 │ alpha");
        assert_eq!(lines[1], " 12 │ beta");
    }
}
