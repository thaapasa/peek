//! The SQL info section: a Content block (shared text stats) plus a SQL block,
//! driven by one [`SqlView`] that derives both `serde::Serialize` (JSON) and
//! [`InfoView`](crate::info::InfoView) (themed print). [`SqlInfo`] stays the
//! gather struct; the view projects it.
//!
//! The Content block is print-only (`#[serde(skip)]` — JSON keeps just the SQL
//! stats); the SQL block flattens into the top-level object. Each created-
//! object kind shows as a count row plus an indented muted `Names` row (an
//! [`ObjectList`] sub-view) while serializing as a names array.

use serde::{Serialize, Serializer};

use crate::info::{InfoNode, InfoValue, Value, paint_count, render_info};
use crate::types::sql::info::{SqlDialect, SqlInfo, SqlStats};
use crate::types::text::info_render::TextView;
use peek_theme::PeekTheme;

const NAME_LIST_LIMIT: usize = 8;

/// Themed terminal SQL section (Content + SQL blocks).
pub fn render_section(lines: &mut Vec<String>, info: &SqlInfo, theme: &PeekTheme) {
    render_info(lines, &SqlView::from(info), theme);
}

/// Typed `--info --json` view, nested under `"sql"`. Carries the SQL stats
/// only — the shared text stats are a print concern.
pub fn json_section(info: &SqlInfo) -> (&'static str, serde_json::Value) {
    (
        "sql",
        serde_json::to_value(SqlView::from(info)).expect("sql info view serializes"),
    )
}

#[derive(Serialize, crate::info::InfoView)]
struct SqlView {
    // Print-only Content block; JSON keeps just the SQL stats.
    #[info(nest)]
    #[serde(skip)]
    content: TextView,
    #[info(nest)]
    #[serde(flatten)]
    sql: SqlSection,
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "SQL")]
struct SqlSection {
    #[info(label = "Dialect")]
    dialect: SqlDialect,
    #[info(label = "Statements")]
    statement_count: Value,
    #[info(label = "  DDL", skip_if_zero)]
    ddl_count: Value,
    #[info(label = "  DML", skip_if_zero)]
    dml_count: Value,
    #[info(label = "  DQL", skip_if_zero)]
    dql_count: Value,
    #[info(label = "  TCL", skip_if_zero)]
    tcl_count: Value,
    #[info(label = "  Other", skip_if_zero)]
    other_count: Value,
    #[info(nest)]
    #[serde(
        rename = "created_tables",
        skip_serializing_if = "ObjectList::is_empty"
    )]
    tables: ObjectList,
    #[info(nest)]
    #[serde(rename = "created_views", skip_serializing_if = "ObjectList::is_empty")]
    views: ObjectList,
    #[info(nest)]
    #[serde(
        rename = "created_indexes",
        skip_serializing_if = "ObjectList::is_empty"
    )]
    indexes: ObjectList,
    #[info(nest)]
    #[serde(
        rename = "created_functions",
        skip_serializing_if = "ObjectList::is_empty"
    )]
    functions: ObjectList,
    #[info(nest)]
    #[serde(
        rename = "created_triggers",
        skip_serializing_if = "ObjectList::is_empty"
    )]
    triggers: ObjectList,
    #[info(label = "PL/pgSQL", skip_if = "DollarQuoted::clear")]
    has_dollar_quoted: DollarQuoted,
    #[info(label = "Comment Lines", skip_if_zero)]
    comment_lines: Value,
}

impl From<&SqlInfo> for SqlView {
    fn from(info: &SqlInfo) -> Self {
        let s: &SqlStats = &info.stats;
        let list = |label, names: &[String]| ObjectList {
            label,
            names: names.to_vec(),
        };
        SqlView {
            content: TextView::from(&info.text),
            sql: SqlSection {
                dialect: s.dialect,
                statement_count: Value::count(s.statement_count as u64),
                ddl_count: Value::count(s.ddl_count as u64),
                dml_count: Value::count(s.dml_count as u64),
                dql_count: Value::count(s.dql_count as u64),
                tcl_count: Value::count(s.tcl_count as u64),
                other_count: Value::count(s.other_count as u64),
                tables: list("Tables", &s.created_tables),
                views: list("Views", &s.created_views),
                indexes: list("Indexes", &s.created_indexes),
                functions: list("Functions", &s.created_functions),
                triggers: list("Triggers", &s.created_triggers),
                has_dollar_quoted: DollarQuoted(s.has_dollar_quoted),
                comment_lines: Value::count(s.comment_lines as u64),
            },
        }
    }
}

/// A created-object kind: print is a count row plus an indented muted `Names`
/// row (names truncated to [`NAME_LIST_LIMIT`]); JSON is the names array.
struct ObjectList {
    label: &'static str,
    names: Vec<String>,
}

impl ObjectList {
    fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

impl crate::info::InfoView for ObjectList {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        if self.names.is_empty() {
            return Vec::new();
        }
        let shown: Vec<String> = self.names.iter().take(NAME_LIST_LIMIT).cloned().collect();
        let mut joined = shown.join(", ");
        if self.names.len() > NAME_LIST_LIMIT {
            joined.push_str(&format!(", … (+{})", self.names.len() - NAME_LIST_LIMIT));
        }
        vec![
            InfoNode::Row {
                label: self.label.into(),
                value: paint_count(self.names.len(), theme),
            },
            InfoNode::Row {
                label: "  Names".into(),
                value: theme.paint_muted(&joined),
            },
        ]
    }
}

impl Serialize for ObjectList {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.names.serialize(ser)
    }
}

/// `$$`-quoted body flag: a JSON bool always, but a print row (value
/// `inline $$ block`) only when set.
struct DollarQuoted(bool);

impl DollarQuoted {
    fn clear(&self) -> bool {
        !self.0
    }
}

impl InfoValue for DollarQuoted {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_value("inline $$ block")
    }
}

impl Serialize for DollarQuoted {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_bool(self.0)
    }
}

impl InfoValue for SqlDialect {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_value(dialect_label(*self))
    }
}

impl Serialize for SqlDialect {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(dialect_token(*self))
    }
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

fn dialect_token(d: SqlDialect) -> &'static str {
    match d {
        SqlDialect::Generic => "generic",
        SqlDialect::PostgreSql => "postgresql",
        SqlDialect::MySql => "mysql",
        SqlDialect::Sqlite => "sqlite",
        SqlDialect::TSql => "tsql",
    }
}
