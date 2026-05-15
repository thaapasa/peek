//! Render the CSV info section.

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::info::{ColumnStats, CsvStats, delimiter_label};

pub fn render_section(lines: &mut Vec<String>, stats: &CsvStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, stats.format.label(), theme);

    push_field(
        lines,
        "Delimiter",
        &theme.paint_accent(delimiter_label(stats.delimiter)),
        theme,
    );
    push_field(lines, "Encoding", &theme.paint_value(stats.encoding), theme);
    if stats.has_bom {
        push_field(lines, "BOM", &theme.paint_muted("yes"), theme);
    }
    push_field(
        lines,
        "Header",
        &theme.paint_value(if stats.header_detected {
            "detected"
        } else {
            "none"
        }),
        theme,
    );

    // Record count: `≥ N (sampled)` while partial, exact otherwise.
    let record_label = match stats.total_records {
        Some(n) => paint_count(n, theme),
        None => format!(
            "{} {}",
            paint_count(stats.loaded_records, theme),
            theme.paint_muted("(partial)")
        ),
    };
    push_field(lines, "Records", &record_label, theme);
    push_field(
        lines,
        "Columns",
        &paint_count(stats.columns.len(), theme),
        theme,
    );
    if stats.malformed_count > 0 {
        push_field(
            lines,
            "Malformed",
            &theme.paint(
                &crate::info::thousands_sep(stats.malformed_count as u64),
                theme.warning,
            ),
            theme,
        );
    }

    if stats.columns.is_empty() {
        return;
    }

    lines.push(String::new());
    push_section_header(lines, "Columns", theme);
    if stats.sampled {
        push_field(
            lines,
            "Sample",
            &theme.paint_muted(&format!("first {} records", stats.loaded_records)),
            theme,
        );
    }
    for (i, col) in stats.columns.iter().enumerate() {
        render_column(lines, i, col, theme);
    }
}

fn render_column(lines: &mut Vec<String>, idx: usize, col: &ColumnStats, theme: &PeekTheme) {
    let header_display = col.header.as_deref().unwrap_or("(no header)");
    let label = format!("{:>2}: {}", idx + 1, header_display);
    let value = format!(
        "{}  width {}  empty {}",
        theme.paint_accent(col.inferred_type.label()),
        theme.paint_value(&col.max_width.to_string()),
        theme.paint_muted(&col.empty_count.to_string()),
    );
    push_field(lines, &label, &value, theme);
}
