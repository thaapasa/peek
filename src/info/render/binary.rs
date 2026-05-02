use super::{push_field, push_section_header};
use crate::theme::PeekTheme;

pub(super) fn render_section(lines: &mut Vec<String>, format: Option<&str>, theme: &PeekTheme) {
    if let Some(fmt) = format {
        lines.push(String::new());
        push_section_header(lines, "Format", theme);
        push_field(lines, "Type", &theme.paint_value(fmt), theme);
    }
}
