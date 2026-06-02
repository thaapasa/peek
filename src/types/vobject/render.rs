//! Shared rendered-view helpers for the calendar / contact read modes.
//!
//! Both renderers emit `Label: value` field rows (wrapped with a hanging
//! indent under the value column) and free-flowing wrapped prose, themed
//! the same way the email renderer themes its header block — so an `.ics`,
//! a `.vcf`, and an `.eml` all read with one visual grammar.

use crate::theme::{PeekTheme, display_width};
use crate::viewer::ui::wrap_styled_words;

/// Emit a `Label: value` row, wrapping a long value with a hanging indent
/// aligned under the value column. A `None` / empty value emits nothing.
pub fn push_field(
    lines: &mut Vec<String>,
    label: &str,
    value: Option<&str>,
    theme: &PeekTheme,
    width: usize,
) {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return;
    };
    let prefix = format!("{label}: ");
    let indent = " ".repeat(display_width(&prefix));
    let budget = width.saturating_sub(display_width(&prefix)).max(1);
    let painted = theme.paint_value(value);
    for (i, chunk) in wrap_styled_words(&painted, budget).into_iter().enumerate() {
        if i == 0 {
            lines.push(format!("{}{chunk}", theme.paint_label(&prefix)));
        } else {
            lines.push(format!("{indent}{chunk}"));
        }
    }
}

/// Emit a multi-line prose block (a `DESCRIPTION` / `NOTE`), word-wrapped
/// to `width`. Embedded newlines (from `\n` escapes) start fresh lines;
/// blank lines are preserved.
pub fn push_prose(lines: &mut Vec<String>, text: &str, theme: &PeekTheme, width: usize) {
    for raw in text.split('\n') {
        if raw.is_empty() {
            lines.push(String::new());
        } else {
            lines.extend(wrap_styled_words(&theme.paint_value(raw), width));
        }
    }
}
