use super::keys::Action;
use crate::theme::{PeekTheme, PeekThemeName};

pub(crate) fn render_help_with_keys(
    theme: &PeekTheme,
    current_theme: PeekThemeName,
    actions: &[(Action, &str)],
) -> Vec<String> {
    let mut lines = Vec::new();

    // Section header
    let rule = "\u{2500}".repeat(28);
    lines.push(format!(
        "{} {} {}",
        theme.paint_muted("\u{2500}\u{2500}"),
        theme.paint_heading("Keyboard Shortcuts"),
        theme.paint_muted(&rule),
    ));

    // Key overhead for alignment (ANSI codes in paint_label)
    let sample_painted = theme.paint_label("x");
    let overhead = sample_painted.len() - 1;
    let key_width = 14 + overhead;

    for (action, desc) in actions {
        lines.push(format!(
            "  {:<width$}{}",
            theme.paint_label(action.label_keys()),
            theme.paint_muted(desc),
            width = key_width,
        ));
    }

    lines.push(String::new());

    // Theme info
    let rule2 = "\u{2500}".repeat(35);
    lines.push(format!(
        "{} {} {}",
        theme.paint_muted("\u{2500}\u{2500}"),
        theme.paint_heading("Theme"),
        theme.paint_muted(&rule2),
    ));
    lines.push(format!(
        "  {:<width$}{}",
        theme.paint_label("Active"),
        theme.paint_value(current_theme.cli_name()),
        width = key_width,
    ));

    lines
}
