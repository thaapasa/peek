//! SQL info shape: text-stats sidecar plus statement / object / dialect
//! scanner output.

use crate::types::text::info::TextStats;

pub struct SqlInfo {
    pub text: TextStats,
    pub stats: SqlStats,
}

pub struct SqlStats {
    pub statement_count: usize,
    pub ddl_count: usize,
    pub dml_count: usize,
    pub dql_count: usize,
    pub tcl_count: usize,
    pub other_count: usize,
    /// Distinct objects created/altered/dropped, by kind, in first-seen order.
    pub created_tables: Vec<String>,
    pub created_views: Vec<String>,
    pub created_indexes: Vec<String>,
    pub created_functions: Vec<String>,
    pub created_triggers: Vec<String>,
    /// Comment lines (any of `--`, `#`, `/* … */`).
    pub comment_lines: usize,
    /// Heuristic dialect guess.
    pub dialect: SqlDialect,
    /// True if any `$$ … $$` body found (PL/pgSQL or Postgres anonymous block).
    pub has_dollar_quoted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SqlDialect {
    Generic,
    PostgreSql,
    MySql,
    Sqlite,
    TSql,
}
