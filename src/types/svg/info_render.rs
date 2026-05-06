use crate::info::{SvgAnimationStats, paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

#[allow(clippy::too_many_arguments)]
pub fn render_section(
    lines: &mut Vec<String>,
    view_box: Option<&str>,
    declared_width: Option<&str>,
    declared_height: Option<&str>,
    path_count: usize,
    group_count: usize,
    rect_count: usize,
    circle_count: usize,
    text_count: usize,
    has_script: bool,
    has_external_href: bool,
    animation: Option<&SvgAnimationStats>,
    animation_warning: Option<&str>,
    theme: &PeekTheme,
) {
    lines.push(String::new());
    push_section_header(lines, "SVG", theme);
    if let Some(vb) = view_box {
        push_field(lines, "viewBox", &theme.paint_value(vb), theme);
    }
    if let Some(w) = declared_width {
        push_field(lines, "Width", &theme.paint_value(w), theme);
    }
    if let Some(h) = declared_height {
        push_field(lines, "Height", &theme.paint_value(h), theme);
    }
    if path_count > 0 {
        push_field(lines, "Paths", &paint_count(path_count, theme), theme);
    }
    if group_count > 0 {
        push_field(lines, "Groups", &paint_count(group_count, theme), theme);
    }
    if rect_count > 0 {
        push_field(lines, "Rects", &paint_count(rect_count, theme), theme);
    }
    if circle_count > 0 {
        push_field(lines, "Circles", &paint_count(circle_count, theme), theme);
    }
    if text_count > 0 {
        push_field(lines, "Text Elems", &paint_count(text_count, theme), theme);
    }
    if has_script {
        push_field(lines, "Script", &theme.paint(" yes", theme.warning), theme);
    }
    if has_external_href {
        push_field(
            lines,
            "External ref",
            &theme.paint(" yes", theme.warning),
            theme,
        );
    }
    if let Some(a) = animation {
        let dur_s = a.total_duration_ms as f64 / 1000.0;
        let label = if a.infinite { "looping" } else { "one-shot" };
        let value = format!("{} frames, {:.2}s ({label})", a.frame_count, dur_s);
        push_field(lines, "Animation", &theme.paint_value(&value), theme);
    } else if let Some(reason) = animation_warning {
        let painted = theme.paint(&format!(" {reason}"), theme.warning);
        push_field(lines, "Animation", &painted, theme);
    }
}
