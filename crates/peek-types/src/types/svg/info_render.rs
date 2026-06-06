use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;
use crate::types::svg::info::{SvgAnimationStats, SvgStats};
use crate::types::text::info_render::push_text_stats;

pub fn render_section(lines: &mut Vec<String>, stats: &SvgStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "SVG", theme);
    if let Some(vb) = &stats.view_box {
        push_field(lines, "viewBox", &theme.paint_value(vb), theme);
    }
    if let Some(w) = &stats.declared_width {
        push_field(lines, "Width", &theme.paint_value(w), theme);
    }
    if let Some(h) = &stats.declared_height {
        push_field(lines, "Height", &theme.paint_value(h), theme);
    }
    if stats.path_count > 0 {
        push_field(lines, "Paths", &paint_count(stats.path_count, theme), theme);
    }
    if stats.group_count > 0 {
        push_field(
            lines,
            "Groups",
            &paint_count(stats.group_count, theme),
            theme,
        );
    }
    if stats.rect_count > 0 {
        push_field(lines, "Rects", &paint_count(stats.rect_count, theme), theme);
    }
    if stats.circle_count > 0 {
        push_field(
            lines,
            "Circles",
            &paint_count(stats.circle_count, theme),
            theme,
        );
    }
    if stats.text_count > 0 {
        push_field(
            lines,
            "Text Elems",
            &paint_count(stats.text_count, theme),
            theme,
        );
    }
    if stats.has_script {
        push_field(lines, "Script", &theme.paint(" yes", theme.warning), theme);
    }
    if stats.has_external_href {
        push_field(
            lines,
            "External ref",
            &theme.paint(" yes", theme.warning),
            theme,
        );
    }
    push_animation_row(
        lines,
        stats.animation.as_ref(),
        stats.animation_warning.as_deref(),
        theme,
    );

    lines.push(String::new());
    push_section_header(lines, "Source", theme);
    push_text_stats(lines, &stats.text, theme);
}

/// Typed `--info --json` encoding of the SVG section. Optional dimension
/// strings and element counts are omitted when absent / zero; the text-stats
/// sidecar is nested under `source`.
pub fn json_section(stats: &SvgStats) -> (&'static str, serde_json::Value) {
    let mut obj = serde_json::json!({
        "path_count": stats.path_count,
        "group_count": stats.group_count,
        "rect_count": stats.rect_count,
        "circle_count": stats.circle_count,
        "text_count": stats.text_count,
        "has_script": stats.has_script,
        "has_external_href": stats.has_external_href,
    });
    if let Some(ref vb) = stats.view_box {
        obj["view_box"] = serde_json::json!(vb);
    }
    if let Some(ref w) = stats.declared_width {
        obj["declared_width"] = serde_json::json!(w);
    }
    if let Some(ref h) = stats.declared_height {
        obj["declared_height"] = serde_json::json!(h);
    }
    if let Some(ref a) = stats.animation {
        obj["animation"] = serde_json::json!({
            "frame_count": a.frame_count,
            "total_duration_ms": a.total_duration_ms,
            "infinite": a.infinite,
        });
    }
    if let Some(ref warn) = stats.animation_warning {
        obj["animation_warning"] = serde_json::json!(warn);
    }
    let (_, source) = crate::types::text::info_render::json_section(&stats.text);
    obj["source"] = source;
    ("svg", obj)
}

fn push_animation_row(
    lines: &mut Vec<String>,
    animation: Option<&SvgAnimationStats>,
    animation_warning: Option<&str>,
    theme: &PeekTheme,
) {
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
