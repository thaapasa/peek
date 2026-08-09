use peek_theme::{PeekTheme, PeekThemeName};

use super::keys::{Action, HelpEntry};

/// One labelled block of the help screen — the global keys, or one
/// mode's extras. The viewer composes a section per mode so the help
/// screen makes clear which keys belong to which view (the same screen
/// lists every mode the file has, not just the active one).
pub struct HelpSection {
    pub title: String,
    pub entries: Vec<HelpEntry>,
}

pub fn render_help_with_keys(
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
        let rule = "\u{2500}".repeat(36usize.saturating_sub(peek_theme::display_width(title)));
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
            // joined with " / ". Consecutive `ZoomPreset(n)` actions
            // collapse to a `1-9` range so a 9-preset bundle stays one
            // compact label.
            let keys = format_group_keys(group);
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

/// Render a group of actions as one " / "-joined key label, collapsing
/// runs of consecutive `Action::ZoomPreset(n)` (where each `n` increments
/// by 1) into a single `start-end` range. Everything else falls through
/// to `Action::label_keys`.
fn format_group_keys(group: &[Action]) -> String {
    let mut labels: Vec<String> = Vec::new();
    let mut i = 0;
    while i < group.len() {
        if let Action::ZoomPreset(start) = group[i] {
            let mut end = start;
            let mut j = i + 1;
            while j < group.len() {
                if let Action::ZoomPreset(n) = group[j]
                    && n == end + 1
                {
                    end = n;
                    j += 1;
                } else {
                    break;
                }
            }
            if end > start {
                labels.push(format!("{start} - {end}"));
                i = j;
                continue;
            }
        }
        labels.push(group[i].label_keys());
        i += 1;
    }
    labels.join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_group_keys_collapses_zoom_preset_range() {
        let g = [
            Action::ZoomPreset(1),
            Action::ZoomPreset(2),
            Action::ZoomPreset(3),
            Action::ZoomPreset(4),
            Action::ZoomPreset(5),
            Action::ZoomPreset(6),
            Action::ZoomPreset(7),
            Action::ZoomPreset(8),
            Action::ZoomPreset(9),
        ];
        assert_eq!(format_group_keys(&g), "1 - 9");
    }

    #[test]
    fn format_group_keys_single_preset_renders_as_digit() {
        assert_eq!(format_group_keys(&[Action::ZoomPreset(3)]), "3");
    }

    #[test]
    fn format_group_keys_non_consecutive_presets_stay_joined() {
        let g = [Action::ZoomPreset(1), Action::ZoomPreset(3)];
        assert_eq!(format_group_keys(&g), "1 / 3");
    }

    #[test]
    fn format_group_keys_falls_through_for_non_preset_groups() {
        let g = [Action::ZoomIn, Action::ZoomOut];
        // Both render via the existing label_keys path, joined with " / ".
        assert_eq!(format_group_keys(&g), "+, = / -");
    }
}
