use crate::info::{SqlDialect, SqlStats, paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

const NAME_LIST_LIMIT: usize = 8;

pub fn render_section(lines: &mut Vec<String>, stats: &SqlStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "SQL", theme);

    push_field(
        lines,
        "Dialect",
        &theme.paint_value(dialect_label(stats.dialect)),
        theme,
    );

    push_field(
        lines,
        "Statements",
        &paint_count(stats.statement_count, theme),
        theme,
    );
    push_kind(lines, "  DDL", stats.ddl_count, theme);
    push_kind(lines, "  DML", stats.dml_count, theme);
    push_kind(lines, "  DQL", stats.dql_count, theme);
    push_kind(lines, "  TCL", stats.tcl_count, theme);
    push_kind(lines, "  Other", stats.other_count, theme);

    push_objects(lines, "Tables", &stats.created_tables, theme);
    push_objects(lines, "Views", &stats.created_views, theme);
    push_objects(lines, "Indexes", &stats.created_indexes, theme);
    push_objects(lines, "Functions", &stats.created_functions, theme);
    push_objects(lines, "Triggers", &stats.created_triggers, theme);

    if stats.has_dollar_quoted {
        push_field(
            lines,
            "PL/pgSQL",
            &theme.paint_value("inline $$ block"),
            theme,
        );
    }

    if stats.comment_lines > 0 {
        push_field(
            lines,
            "Comment Lines",
            &paint_count(stats.comment_lines, theme),
            theme,
        );
    }
}

fn push_kind(lines: &mut Vec<String>, label: &str, count: usize, theme: &PeekTheme) {
    if count > 0 {
        push_field(lines, label, &paint_count(count, theme), theme);
    }
}

fn push_objects(lines: &mut Vec<String>, label: &str, names: &[String], theme: &PeekTheme) {
    if names.is_empty() {
        return;
    }
    push_field(lines, label, &paint_count(names.len(), theme), theme);
    let shown: Vec<String> = names.iter().take(NAME_LIST_LIMIT).cloned().collect();
    let mut joined = shown.join(", ");
    if names.len() > NAME_LIST_LIMIT {
        joined.push_str(&format!(", … (+{})", names.len() - NAME_LIST_LIMIT));
    }
    push_field(lines, "  Names", &theme.paint_muted(&joined), theme);
}

fn dialect_label(d: SqlDialect) -> &'static str {
    match d {
        SqlDialect::Generic => "generic",
        SqlDialect::PostgreSql => "PostgreSQL",
        SqlDialect::MySql => "MySQL",
        SqlDialect::Sqlite => "SQLite",
        SqlDialect::TSql => "T-SQL",
    }
}
