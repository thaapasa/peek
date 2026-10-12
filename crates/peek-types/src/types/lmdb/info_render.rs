//! Renders [`LmdbInfo`] through one [`LmdbView`] driving both the themed
//! terminal output and `--info --json`. Bespoke (not derived): one block
//! per database, a variable count. On a read error only a `Status` row /
//! `error` key shows.

use peek_theme::PeekTheme;
use serde::{Serialize, Serializer};

use super::cells::MAIN_LABEL;
use super::format::DbRecord;
use super::info::LmdbInfo;
use crate::info::{InfoNode, format_size_human, paint_count, render_info, thousands_sep};

/// Named-DB blocks shown before the rest collapse into one summary row.
const MAX_DB_BLOCKS: usize = 32;

pub fn render_section(lines: &mut Vec<String>, info: &LmdbInfo, theme: &PeekTheme) {
    render_info(lines, &LmdbView(info), theme);
}

pub fn json_section(info: &LmdbInfo) -> (&'static str, serde_json::Value) {
    (
        "lmdb",
        serde_json::to_value(LmdbView(info)).expect("lmdb info view serializes"),
    )
}

struct LmdbView<'a>(&'a LmdbInfo);

fn row(label: impl Into<String>, value: String) -> InfoNode {
    InfoNode::Row {
        label: label.into().into(),
        value,
    }
}

fn pages(n: u64) -> String {
    let unit = if n == 1 { "page" } else { "pages" };
    format!("{} {unit}", thousands_sep(n))
}

fn db_block(title: String, db: &DbRecord, theme: &PeekTheme) -> InfoNode {
    let mut body = vec![
        row("Entries", paint_count(db.entries as usize, theme)),
        row("Depth", theme.paint_value(&db.depth.to_string())),
        row(
            "Pages",
            theme.paint_value(&format!(
                "{} branch · {} leaf · {} overflow",
                thousands_sep(db.branch_pages),
                thousands_sep(db.leaf_pages),
                thousands_sep(db.overflow_pages)
            )),
        ),
    ];
    let flags = db.flag_names();
    if !flags.is_empty() {
        body.push(row("Flags", theme.paint_value(&flags.join(", "))));
    }
    InfoNode::Block { title, body }
}

impl crate::info::InfoView for LmdbView<'_> {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let Some(s) = &self.0.stats else {
            let msg = self
                .0
                .error
                .as_deref()
                .unwrap_or("could not read LMDB file");
            return vec![InfoNode::Block {
                title: "LMDB".to_string(),
                body: vec![row("Status", theme.paint_warning(msg))],
            }];
        };
        let mut body = vec![
            row("Format", theme.paint_value(&format!("LMDB {}", s.format))),
            row(
                "Page size",
                theme.paint_value(&format_size_human(s.page_size)),
            ),
            row(
                "Map size",
                theme.paint_value(&format_size_human(s.map_size)),
            ),
            row(
                "Used",
                theme.paint_value(&format!(
                    "{} · {}",
                    pages(s.used_pages),
                    format_size_human(s.used_pages.saturating_mul(s.page_size))
                )),
            ),
            row("Last txn", theme.paint_value(&thousands_sep(s.last_txnid))),
            row("Active meta", theme.paint_value(&s.active_meta.to_string())),
            row("Databases", paint_count(s.named.len(), theme)),
            row("Free list", theme.paint_value(&pages(s.free.total_pages()))),
        ];
        if let Some(note) = &s.named_note {
            body.push(row("Note", theme.paint_warning(note)));
        }
        let mut nodes = vec![
            InfoNode::Block {
                title: "LMDB".to_string(),
                body,
            },
            db_block(format!("Database {MAIN_LABEL}"), &s.main, theme),
        ];
        for (name, db) in s.named.iter().take(MAX_DB_BLOCKS) {
            nodes.push(db_block(format!("Database {name}"), db, theme));
        }
        if s.named.len() > MAX_DB_BLOCKS {
            nodes.push(InfoNode::Block {
                title: "More databases".to_string(),
                body: vec![row(
                    "Not shown",
                    paint_count(s.named.len() - MAX_DB_BLOCKS, theme),
                )],
            });
        }
        nodes
    }
}

fn db_json(name: Option<&str>, db: &DbRecord) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "entries": db.entries,
        "depth": db.depth,
        "branch_pages": db.branch_pages,
        "leaf_pages": db.leaf_pages,
        "overflow_pages": db.overflow_pages,
        "flags": db.flag_names(),
    })
}

impl Serialize for LmdbView<'_> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let Some(s) = &self.0.stats else {
            let msg = self
                .0
                .error
                .as_deref()
                .unwrap_or("could not read LMDB file");
            return serde_json::json!({ "error": msg }).serialize(ser);
        };
        let databases: Vec<serde_json::Value> = s
            .named
            .iter()
            .map(|(name, db)| db_json(Some(name), db))
            .collect();
        serde_json::json!({
            "format": s.format,
            "page_size": s.page_size,
            "map_size": s.map_size,
            "used_pages": s.used_pages,
            "last_txnid": s.last_txnid,
            "active_meta": s.active_meta,
            "free_list_pages": s.free.total_pages(),
            "main": db_json(None, &s.main),
            "databases": databases,
            "note": s.named_note,
        })
        .serialize(ser)
    }
}
