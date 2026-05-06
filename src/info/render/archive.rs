use super::{paint_count, push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;

#[allow(clippy::too_many_arguments)]
pub(super) fn render_section(
    lines: &mut Vec<String>,
    format_name: &str,
    entry_count: usize,
    file_count: usize,
    dir_count: usize,
    total_uncompressed_size: u64,
    error: Option<&str>,
    theme: &PeekTheme,
) {
    lines.push(String::new());
    push_section_header(lines, "Archive", theme);
    push_field(lines, "Format", &theme.paint_value(format_name), theme);

    if let Some(err) = error {
        push_field(lines, "Status", &theme.paint_warning(err), theme);
        return;
    }

    push_field(lines, "Entries", &paint_count(entry_count, theme), theme);
    push_field(lines, "Files", &paint_count(file_count, theme), theme);
    push_field(lines, "Directories", &paint_count(dir_count, theme), theme);
    push_field(
        lines,
        "Total size",
        &theme.paint_value(&format!("{} bytes", thousands_sep(total_uncompressed_size))),
        theme,
    );
}
