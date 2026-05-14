use super::keys::HelpEntry;
use crate::theme::{PeekTheme, PeekThemeName};

/// One labelled block of the help screen — the global keys, or one
/// mode's extras. The viewer composes a section per mode so the help
/// screen makes clear which keys belong to which view (the same screen
/// lists every mode the file has, not just the active one).
pub(crate) struct HelpSection {
    pub title: String,
    pub entries: Vec<HelpEntry>,
}

pub(crate) fn render_help_with_keys(
    theme: &PeekTheme,
    current_theme: PeekThemeName,
    sections: &[HelpSection],
) -> Vec<String> {
    let mut lines = Vec::new();

    // Key overhead for alignment (ANSI codes in paint_label).
    let sample_painted = theme.paint_label("x");
    let overhead = sample_painted.len() - 1;
    let key_width = 19 + overhead;

    let section_header = |lines: &mut Vec<String>, title: &str| {
        let rule = "\u{2500}".repeat(36usize.saturating_sub(title.len()));
        lines.push(format!(
            "{} {} {}",
            theme.paint_muted("\u{2500}\u{2500}"),
            theme.paint_heading(title),
            theme.paint_muted(&rule),
        ));
    };

    for (i, section) in sections.iter().enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        section_header(&mut lines, &section.title);
        for (group, desc) in &section.entries {
            // A help entry can bundle several actions under one
            // description (e.g. next / previous) — render their keys
            // joined with " / ".
            let keys = group
                .iter()
                .map(|a| a.label_keys())
                .collect::<Vec<_>>()
                .join(" / ");
            lines.push(format!(
                "  {:<width$}{}",
                theme.paint_label(&keys),
                theme.paint_muted(desc),
                width = key_width,
            ));
        }
    }

    // Theme info.
    lines.push(String::new());
    section_header(&mut lines, "Theme");
    lines.push(format!(
        "  {:<width$}{}",
        theme.paint_label("Active"),
        theme.paint_value(current_theme.cli_name()),
        width = key_width,
    ));
    lines.push(format!(
        "  {:<width$}{}",
        theme.paint_label("Color mode"),
        theme.paint_value(theme.style_mode.cli_name()),
        width = key_width,
    ));

    lines
}
