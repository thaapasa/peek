//! `ManualTocSource`: the manual's table of contents as a [`ListSource`].
//!
//! `TreeListSource` doesn't fit: it shows path segments, not titles, and
//! never extracts a row that has children, while a manual page such as
//! "Overview" is both a page and the parent of the file-type chapters.
//! Rows follow `SUMMARY.md` order. Part headings are top-level group rows
//! (selectable for parent navigation, not extractable); chapters nest
//! under them with the same tree connectors the archive TOC uses.

use peek_foundation::viewer::listing::row::{self, SizeCell};
use peek_foundation::viewer::listing::{ListSource, NameCell, NameStyle, RowCells};
use peek_foundation::viewer::modes::{ExtractTarget, RenderCtx};
use peek_theme::PeekTheme;

use crate::{ENTRIES, TocEntry};

struct Node {
    title: &'static str,
    /// Extract key; `None` for part headings.
    path: Option<&'static str>,
    size: u64,
    children: Vec<Node>,
}

struct Row {
    prefix: String,
    title: &'static str,
    path: Option<&'static str>,
    size: u64,
    parent: Option<usize>,
}

pub struct ManualTocSource {
    rows: Vec<Row>,
}

impl ManualTocSource {
    pub fn new() -> Self {
        let mut rows = Vec::new();
        flatten(&build_tree(ENTRIES), None, None, &mut rows);
        Self { rows }
    }
}

impl Default for ManualTocSource {
    fn default() -> Self {
        Self::new()
    }
}

fn build_tree(entries: &[TocEntry]) -> Vec<Node> {
    let mut roots: Vec<Node> = Vec::new();
    let mut part: Option<usize> = None;
    for entry in entries {
        match entry {
            TocEntry::Part(title) => {
                roots.push(Node {
                    title,
                    path: None,
                    size: 0,
                    children: Vec::new(),
                });
                part = Some(roots.len() - 1);
            }
            TocEntry::Chapter(c) => {
                let node = Node {
                    title: c.title,
                    path: Some(c.path),
                    size: c.body.len() as u64,
                    children: Vec::new(),
                };
                let list = match part {
                    Some(p) => &mut roots[p].children,
                    None => &mut roots,
                };
                push_at(list, c.depth, node);
            }
        }
    }
    roots
}

/// Push `node` `depth` levels down, under the last node of each level.
fn push_at(list: &mut Vec<Node>, depth: usize, node: Node) {
    match list.last_mut() {
        Some(last) if depth > 0 => push_at(&mut last.children, depth - 1, node),
        _ => list.push(node),
    }
}

/// Depth-first rows. Top-level rows sit flush left (`prefix` is `None`);
/// below that, rows get the same tree connectors as `TreeListSource`.
fn flatten(nodes: &[Node], prefix: Option<&str>, parent: Option<usize>, rows: &mut Vec<Row>) {
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i + 1 == nodes.len();
        let row_prefix = prefix.map_or_else(String::new, |p| {
            format!("{p}{}", row::tree_connector(is_last))
        });
        rows.push(Row {
            prefix: row_prefix,
            title: node.title,
            path: node.path,
            size: node.size,
            parent,
        });
        let child_prefix = prefix.map_or_else(String::new, |p| {
            format!("{p}{}", row::tree_continuation(is_last))
        });
        flatten(
            &node.children,
            Some(&child_prefix),
            Some(rows.len() - 1),
            rows,
        );
    }
}

impl ListSource for ManualTocSource {
    fn len(&self) -> usize {
        self.rows.len()
    }

    fn parent(&self, idx: usize) -> Option<usize> {
        self.rows[idx].parent
    }

    fn selectable(&self, _idx: usize) -> bool {
        true
    }

    fn name(&self, idx: usize) -> &str {
        self.rows[idx].title
    }

    fn row_cells(&self, idx: usize, ctx: &RenderCtx) -> RowCells {
        let row = &self.rows[idx];
        let size = match row.path {
            Some(_) => row::paint_size(
                &row::format_size(SizeCell::Bytes(row.size), ctx.render_opts.human_sizes),
                row.size,
                false,
                ctx.peek_theme,
            ),
            None => row::pad_size(""),
        };
        RowCells {
            prefix: row.prefix.clone(),
            left: vec![size],
            name: NameCell {
                text: row.title.to_string(),
                style: match row.path {
                    Some(_) => NameStyle::Plain,
                    None => NameStyle::Heading,
                },
            },
        }
    }

    fn extract_target(&self, idx: usize) -> Option<ExtractTarget> {
        self.rows[idx]
            .path
            .map(|p| ExtractTarget::EntryPath(p.to_string()))
    }

    /// `path<TAB>title`, so a script can `cut -f1` the keys for `-x`. Part
    /// headings have no key and are left out.
    fn flat_line(&self, idx: usize, theme: &PeekTheme) -> Option<String> {
        let row = &self.rows[idx];
        let path = row.path?;
        Some(format!(
            "{}\t{}",
            theme.paint(path, theme.foreground),
            row.title
        ))
    }

    fn source_label(&self) -> &str {
        "Manual"
    }

    fn initial_selection(&self) -> Option<usize> {
        self.rows.iter().position(|r| r.path.is_some())
    }
}
