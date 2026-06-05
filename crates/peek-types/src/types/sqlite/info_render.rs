//! Renders [`SqliteInfo`] into themed terminal lines.
//!
//! Shape mirrors [`crate::types::objfile::info_render`]: file-level
//! pragmas first, then schema counts, then a short "Biggest tables"
//! section when the database carries any. Optional fields (user /
//! application version, empty entity groups) only appear when
//! non-default to keep the section compact on small DBs.

use super::info::SqliteInfo;
use crate::info::{paint_count, push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;

pub fn render_section(lines: &mut Vec<String>, info: &SqliteInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "SQLite", theme);

    let Some(stats) = &info.stats else {
        let msg = info
            .error
            .as_deref()
            .unwrap_or("could not read SQLite database");
        push_field(lines, "Status", &theme.paint_warning(msg), theme);
        return;
    };

    push_field(
        lines,
        "Page size",
        &theme.paint_value(&format!("{} bytes", thousands_sep(stats.page_size as u64))),
        theme,
    );
    push_field(
        lines,
        "Pages",
        &theme.paint_value(&thousands_sep(stats.page_count as u64)),
        theme,
    );
    push_field(
        lines,
        "Encoding",
        &theme.paint_value(&stats.encoding),
        theme,
    );
    push_field(
        lines,
        "Journal mode",
        &theme.paint_value(&stats.journal_mode),
        theme,
    );
    push_field(
        lines,
        "Schema vsn",
        &theme.paint_value(&stats.schema_version.to_string()),
        theme,
    );
    if stats.user_version != 0 {
        push_field(
            lines,
            "User vsn",
            &theme.paint_value(&stats.user_version.to_string()),
            theme,
        );
    }
    if stats.application_id != 0 {
        push_field(
            lines,
            "App ID",
            &theme.paint_value(&format!("0x{:08x}", stats.application_id as u32)),
            theme,
        );
    }
    let integrity = if stats.integrity_ok {
        theme.paint_value("ok")
    } else {
        theme.paint_warning("FAILED")
    };
    push_field(lines, "Integrity", &integrity, theme);

    push_field(
        lines,
        "Tables",
        &paint_count(stats.table_count, theme),
        theme,
    );
    if stats.view_count > 0 {
        push_field(lines, "Views", &paint_count(stats.view_count, theme), theme);
    }
    if stats.index_count > 0 {
        push_field(
            lines,
            "Indexes",
            &paint_count(stats.index_count, theme),
            theme,
        );
    }
    if stats.trigger_count > 0 {
        push_field(
            lines,
            "Triggers",
            &paint_count(stats.trigger_count, theme),
            theme,
        );
    }
    push_field(
        lines,
        "Total rows",
        &theme.paint_value(&thousands_sep(stats.total_rows)),
        theme,
    );

    if !stats.top_tables.is_empty() {
        lines.push(String::new());
        push_section_header(lines, "Biggest tables", theme);
        for (name, count) in &stats.top_tables {
            push_field(
                lines,
                name,
                &theme.paint_value(&format!("{} rows", thousands_sep(*count))),
                theme,
            );
        }
    }
}
