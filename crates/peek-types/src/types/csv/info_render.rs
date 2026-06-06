//! Render the CSV info section.

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::info::{ColumnStats, ColumnType, CsvStats, delimiter_label};

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

    // Record count: `N (partial)` while still seed-sampled, exact once
    // a count pass has reached EOF.
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

/// Typed `--info --json` encoding of the CSV section. Record / column
/// counts are raw numbers; `total_records` is omitted while only a
/// partial scan has run (mirrors the `(partial)` render qualifier).
pub fn json_section(stats: &CsvStats) -> (&'static str, serde_json::Value) {
    let columns: Vec<serde_json::Value> = stats
        .columns
        .iter()
        .map(|col| {
            let mut c = serde_json::json!({
                "inferred_type": column_type_token(col.inferred_type),
                "empty_count": col.empty_count,
                "max_width": col.max_width,
            });
            if let Some(ref header) = col.header {
                c["header"] = serde_json::json!(header);
            }
            c
        })
        .collect();

    let mut obj = serde_json::json!({
        "delimiter": delimiter_label(stats.delimiter),
        "encoding": stats.encoding,
        "has_bom": stats.has_bom,
        "header_detected": stats.header_detected,
        "loaded_records": stats.loaded_records,
        "malformed_count": stats.malformed_count,
        "sampled": stats.sampled,
        "column_count": stats.columns.len(),
        "columns": columns,
    });
    if let Some(n) = stats.total_records {
        obj["total_records"] = serde_json::json!(n);
    }
    ("csv", obj)
}

fn column_type_token(t: ColumnType) -> &'static str {
    match t {
        ColumnType::Int => "int",
        ColumnType::Float => "float",
        ColumnType::Bool => "bool",
        ColumnType::Date => "date",
        ColumnType::String => "string",
        ColumnType::Mixed => "mixed",
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
