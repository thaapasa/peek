//! `TreeListSource`: the file-tree [`ListSource`] behind container TOCs —
//! archives, ISO images, embedded-file lists, zip-backed documents
//! (epub / docx / odt), spreadsheets, SQLite schema. Owns the flattened
//! [`TreeRow`] index built from an [`Entry`] tree and paints the perms /
//! size / mtime columns; the engine paints the name column and navigates.
//!
//! The tree is flattened once at construction into a depth-first row list,
//! each row carrying its tree-connector prefix, parent index (for the
//! sticky breadcrumb), and — for files — the slash-joined inner path used
//! as the extract key. Directories are rendered but never selectable.

use super::entry::{Entry, EntryKind, EntryMtime};
use super::row::{self, MTIME_HIDE_BELOW_COLS, SizeCell};
use super::source::{ListSource, NameCell, RowCells};
use crate::theme::PeekTheme;
use crate::viewer::modes::{ExtractTarget, RenderCtx};

pub struct TreeListSource {
    format_name: String,
    /// Pre-flattened tree-walk rows. Populated once at construction;
    /// rendering slices into this without rebuilding.
    rows: Vec<TreeRow>,
    /// Widest mtime cell across all rows, precomputed so the path column
    /// abuts cleanly without recomputing per visible slice on every scroll
    /// (and so the column doesn't jitter as rows enter/leave the viewport).
    mtime_width: usize,
}

/// One rendered row in the TOC. Holds enough metadata to render without
/// traversing the source tree again.
#[derive(Clone)]
pub(super) struct TreeRow {
    /// Composed tree prefix: ancestor segments (`│ ` / `  `) plus this
    /// row's `├╴` / `└╴` connector. Empty for top-level rows.
    pub(super) prefix: String,
    /// Last path segment shown alone — the tree prefix conveys depth.
    pub(super) leaf: String,
    pub(super) is_dir: bool,
    pub(super) size: u64,
    pub(super) mode: Option<u32>,
    pub(super) mtime: Option<EntryMtime>,
    /// Index of the row representing this entry's parent directory, or
    /// `None` for top-level entries. Drives the sticky breadcrumb chain.
    pub(super) parent_row: Option<usize>,
    /// Slash-joined inner path for file rows; `None` for directories.
    /// Used as the extract key and to mark the row selectable.
    pub(super) inner_path: Option<String>,
}

impl TreeListSource {
    pub fn new(format_name: impl Into<String>, entries: Vec<Entry>) -> Self {
        let rows = flatten(&entries);
        let mtime_width = rows
            .iter()
            .map(|r| format_mtime(r.mtime.as_ref(), false).len())
            .max()
            .unwrap_or(0);
        Self {
            format_name: format_name.into(),
            rows,
            mtime_width,
        }
    }
}

impl ListSource for TreeListSource {
    fn len(&self) -> usize {
        self.rows.len()
    }

    fn parent(&self, idx: usize) -> Option<usize> {
        self.rows[idx].parent_row
    }

    fn selectable(&self, idx: usize) -> bool {
        self.rows[idx].inner_path.is_some()
    }

    fn name(&self, idx: usize) -> &str {
        &self.rows[idx].leaf
    }

    fn source_label(&self) -> &str {
        &self.format_name
    }

    fn extract_target(&self, idx: usize) -> Option<ExtractTarget> {
        self.rows[idx]
            .inner_path
            .as_deref()
            .map(|p| ExtractTarget::EntryPath(p.to_string()))
    }

    fn row_cells(&self, idx: usize, ctx: &RenderCtx) -> RowCells {
        let row = &self.rows[idx];
        let theme = ctx.peek_theme;
        let perms = row::format_perms(if row.is_dir { 'd' } else { '-' }, row.mode, row.is_dir);
        let size = row::format_size(if row.is_dir {
            SizeCell::Dir
        } else {
            SizeCell::Bytes(row.size)
        });
        let mut left = vec![
            row::paint_perms(&perms, theme),
            row::paint_size(&size, row.size, row.is_dir, theme),
        ];
        if ctx.term_cols >= MTIME_HIDE_BELOW_COLS {
            let text = format_mtime(row.mtime.as_ref(), ctx.render_opts.utc);
            left.push(row::paint_mtime(&text, self.mtime_width, theme));
        }
        RowCells {
            prefix: row.prefix.clone(),
            left,
            name: NameCell {
                text: row.leaf.clone(),
                is_dir: row.is_dir,
            },
        }
    }

    /// Files only, no tree connectors — each line carries the full
    /// `inner_path` so it can be copy-pasted into `--extract` unedited.
    fn flat_line(&self, idx: usize, theme: &PeekTheme) -> Option<String> {
        let row = &self.rows[idx];
        let path = row.inner_path.as_ref()?;
        let perms = row::format_perms('-', row.mode, false);
        let size = row::format_size(SizeCell::Bytes(row.size));
        Some(row::compose_row(
            &row::paint_perms(&perms, theme),
            &row::paint_size(&size, row.size, false, theme),
            None,
            &theme.paint(path, theme.foreground),
        ))
    }
}

fn flatten(entries: &[Entry]) -> Vec<TreeRow> {
    // Top level: render flush-left without tree connectors. Every
    // depth-1 row would otherwise carry the same `├╴` / `└╴` at
    // column 0, which is visual noise without payload.
    let mut rows = Vec::new();
    for entry in entries {
        let is_dir = entry.is_dir();
        let inner_path = (!is_dir).then(|| entry.name.clone());
        rows.push(TreeRow {
            prefix: String::new(),
            leaf: entry.name.clone(),
            is_dir,
            size: entry.size,
            mode: entry.mode,
            mtime: entry.mtime.clone(),
            parent_row: None,
            inner_path,
        });
        if let EntryKind::Dir { children } = &entry.kind {
            let parent = rows.len() - 1;
            walk(children, Some(parent), "", &entry.name, &mut rows);
        }
    }
    rows
}

fn walk(
    entries: &[Entry],
    parent_row: Option<usize>,
    parent_prefix: &str,
    parent_path: &str,
    rows: &mut Vec<TreeRow>,
) {
    let count = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        let is_last = i + 1 == count;
        // 2-column connectors: corner/tee + thin half-line ("╴", U+2574)
        // that ends at the cell boundary so the leaf abuts cleanly
        // without a separator space. Continuation columns are 2 chars
        // wide as well — vertical bar + space, or two spaces under the
        // last child of a parent.
        let connector = if is_last {
            "\u{2514}\u{2574}"
        } else {
            "\u{251c}\u{2574}"
        };
        let is_dir = entry.is_dir();
        let inner_full = format!("{parent_path}/{}", entry.name);
        let inner_path = (!is_dir).then(|| inner_full.clone());
        rows.push(TreeRow {
            prefix: format!("{parent_prefix}{connector}"),
            leaf: entry.name.clone(),
            is_dir,
            size: entry.size,
            mode: entry.mode,
            mtime: entry.mtime.clone(),
            parent_row,
            inner_path,
        });
        if let EntryKind::Dir { children } = &entry.kind {
            let cont = if is_last { "  " } else { "\u{2502} " };
            let next_prefix = format!("{parent_prefix}{cont}");
            let new_parent = rows.len() - 1;
            walk(children, Some(new_parent), &next_prefix, &inner_full, rows);
        }
    }
}

fn format_mtime(mtime: Option<&EntryMtime>, utc: bool) -> String {
    use std::time::SystemTime;
    let Some(mtime) = mtime else {
        return "-".to_string();
    };
    match mtime {
        EntryMtime::Utc(t) => match t.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => row::format_mtime_epoch(d.as_secs(), utc),
            Err(_) => "-".to_string(),
        },
        EntryMtime::LocalNaive {
            year,
            month,
            day,
            hour,
            minute,
        } => format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rows() -> Vec<TreeRow> {
        let entries = vec![
            Entry {
                name: "sub".into(),
                size: 0,
                mtime: None,
                mode: None,
                kind: EntryKind::Dir {
                    children: vec![
                        Entry {
                            name: "deeper".into(),
                            size: 0,
                            mtime: None,
                            mode: None,
                            kind: EntryKind::Dir {
                                children: vec![Entry {
                                    name: "deep.txt".into(),
                                    size: 4,
                                    mtime: None,
                                    mode: None,
                                    kind: EntryKind::File,
                                }],
                            },
                        },
                        Entry {
                            name: "inner.txt".into(),
                            size: 5,
                            mtime: None,
                            mode: None,
                            kind: EntryKind::File,
                        },
                    ],
                },
            },
            Entry {
                name: "README.txt".into(),
                size: 8,
                mtime: None,
                mode: None,
                kind: EntryKind::File,
            },
        ];
        flatten(&entries)
    }

    #[test]
    fn parent_row_indices_populated() {
        let rows = sample_rows();
        let parents: Vec<Option<usize>> = rows.iter().map(|r| r.parent_row).collect();
        assert_eq!(parents, vec![None, Some(0), Some(1), Some(0), None]);
    }

    #[test]
    fn inner_path_built_for_files_only() {
        let rows = sample_rows();
        let paths: Vec<Option<String>> = rows.iter().map(|r| r.inner_path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                None,                                    // sub/
                None,                                    // sub/deeper/
                Some("sub/deeper/deep.txt".to_string()), // file
                Some("sub/inner.txt".to_string()),       // file
                Some("README.txt".to_string()),          // file
            ]
        );
    }
}
