use super::super::{StructuredStats, TopLevelKind};
use super::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

pub(super) fn render_section(
    lines: &mut Vec<String>,
    format_name: &str,
    stats: Option<&StructuredStats>,
    theme: &PeekTheme,
) {
    lines.push(String::new());
    push_section_header(lines, "Format", theme);
    push_field(lines, "Type", &theme.paint_accent(format_name), theme);
    if let Some(stats) = stats {
        push_structured_stats(lines, stats, theme);
    }
}

fn push_structured_stats(lines: &mut Vec<String>, stats: &StructuredStats, theme: &PeekTheme) {
    let (kind_label, count_label) = match &stats.top_level_kind {
        TopLevelKind::Object => ("Object", "Keys"),
        TopLevelKind::Array => ("Array", "Items"),
        TopLevelKind::Scalar => ("Scalar", "Items"),
        TopLevelKind::Table => ("Table", "Keys"),
        TopLevelKind::MultiDoc(_) => ("Multi-doc", "Top-level"),
        TopLevelKind::Document => ("Document", "Top-level"),
    };
    let kind_text = match &stats.top_level_kind {
        TopLevelKind::MultiDoc(n) => format!("Multi-doc ({n})"),
        _ => kind_label.to_string(),
    };
    push_field(lines, "Top-level", &theme.paint_value(&kind_text), theme);
    if stats.top_level_count > 0 {
        push_field(
            lines,
            count_label,
            &paint_count(stats.top_level_count, theme),
            theme,
        );
    }
    if stats.max_depth > 0 {
        push_field(
            lines,
            "Max Depth",
            &paint_count(stats.max_depth, theme),
            theme,
        );
    }
    if stats.total_nodes > 0 {
        push_field(
            lines,
            "Total Nodes",
            &paint_count(stats.total_nodes, theme),
            theme,
        );
    }
    if let Some(root) = &stats.xml_root {
        push_field(lines, "Root Element", &theme.paint_accent(root), theme);
    }
    if !stats.xml_namespaces.is_empty() {
        for (i, ns) in stats.xml_namespaces.iter().enumerate() {
            let label = if i == 0 { "Namespaces" } else { "" };
            push_field(lines, label, &theme.paint_muted(ns), theme);
        }
    }
}
