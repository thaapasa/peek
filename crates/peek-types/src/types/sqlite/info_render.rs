//! Renders [`SqliteInfo`] via a single [`SqliteView`] that drives both the
//! themed terminal output ([`InfoView`](crate::info::InfoView)) and the
//! `--info --json` form (`serde::Serialize`). On a read error only a `Status`
//! row / `error` key shows; otherwise a SQLite block plus a short
//! "Biggest tables" block.

use peek_theme::PeekTheme;
use serde::{Serialize, Serializer};

use super::info::SqliteInfo;
use crate::info::{InfoNode, paint_count, render_info, thousands_sep};

/// Themed terminal SQLite section.
pub fn render_section(lines: &mut Vec<String>, info: &SqliteInfo, theme: &PeekTheme) {
    render_info(lines, &SqliteView(info), theme);
}

/// Typed `--info --json` view of the SQLite section, nested under `"sqlite"`.
pub fn json_section(info: &SqliteInfo) -> (&'static str, serde_json::Value) {
    (
        "sqlite",
        serde_json::to_value(SqliteView(info)).expect("sqlite info view serializes"),
    )
}

struct SqliteView<'a>(&'a SqliteInfo);

impl crate::info::InfoView for SqliteView<'_> {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let info = self.0;
        let Some(stats) = &info.stats else {
            let msg = info
                .error
                .as_deref()
                .unwrap_or("could not read SQLite database");
            return vec![InfoNode::Block {
                title: "SQLite".to_string(),
                body: vec![InfoNode::Row {
                    label: "Status".into(),
                    value: theme.paint_warning(msg),
                }],
            }];
        };

        let row = |label: &'static str, value: String| InfoNode::Row {
            label: label.into(),
            value,
        };
        let mut body = vec![
            row(
                "Page size",
                theme.paint_value(&format!("{} bytes", thousands_sep(stats.page_size as u64))),
            ),
            row(
                "Pages",
                theme.paint_value(&thousands_sep(stats.page_count as u64)),
            ),
            row("Encoding", theme.paint_value(&stats.encoding)),
            row("Journal mode", theme.paint_value(&stats.journal_mode)),
            row(
                "Schema vsn",
                theme.paint_value(&stats.schema_version.to_string()),
            ),
        ];
        if stats.user_version != 0 {
            body.push(row(
                "User vsn",
                theme.paint_value(&stats.user_version.to_string()),
            ));
        }
        if stats.application_id != 0 {
            body.push(row(
                "App ID",
                theme.paint_value(&format!("0x{:08x}", stats.application_id as u32)),
            ));
        }
        body.push(row(
            "Integrity",
            if stats.integrity_ok {
                theme.paint_value("ok")
            } else {
                theme.paint_warning("FAILED")
            },
        ));
        body.push(row("Tables", paint_count(stats.table_count, theme)));
        if stats.view_count > 0 {
            body.push(row("Views", paint_count(stats.view_count, theme)));
        }
        if stats.index_count > 0 {
            body.push(row("Indexes", paint_count(stats.index_count, theme)));
        }
        if stats.trigger_count > 0 {
            body.push(row("Triggers", paint_count(stats.trigger_count, theme)));
        }
        body.push(row(
            "Total rows",
            theme.paint_value(&thousands_sep(stats.total_rows)),
        ));

        let mut nodes = vec![InfoNode::Block {
            title: "SQLite".to_string(),
            body,
        }];
        if !stats.top_tables.is_empty() {
            let tables = stats
                .top_tables
                .iter()
                .map(|(name, count)| InfoNode::Row {
                    label: name.clone().into(),
                    value: theme.paint_value(&format!("{} rows", thousands_sep(*count))),
                })
                .collect();
            nodes.push(InfoNode::Block {
                title: "Biggest tables".to_string(),
                body: tables,
            });
        }
        nodes
    }
}

impl Serialize for SqliteView<'_> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let info = self.0;
        let Some(stats) = &info.stats else {
            let msg = info
                .error
                .as_deref()
                .unwrap_or("could not read SQLite database");
            return serde_json::json!({ "error": msg }).serialize(ser);
        };
        let top_tables: Vec<serde_json::Value> = stats
            .top_tables
            .iter()
            .map(|(name, rows)| serde_json::json!({ "name": name, "rows": rows }))
            .collect();
        serde_json::json!({
            "page_size": stats.page_size,
            "page_count": stats.page_count,
            "encoding": stats.encoding,
            "schema_version": stats.schema_version,
            "user_version": stats.user_version,
            "application_id": stats.application_id,
            "journal_mode": stats.journal_mode,
            "integrity_ok": stats.integrity_ok,
            "table_count": stats.table_count,
            "view_count": stats.view_count,
            "index_count": stats.index_count,
            "trigger_count": stats.trigger_count,
            "total_rows": stats.total_rows,
            "top_tables": top_tables,
        })
        .serialize(ser)
    }
}
