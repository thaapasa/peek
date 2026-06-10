//! `DirListSource`: the filesystem-directory [`ListSource`]. A flat,
//! one-level listing — every row is selectable (unlike a tree TOC, where
//! only files are), and selecting a row hands its name to the directory
//! extract path: a child file descends into peek, a child directory
//! re-targets the current frame (`ViewerState::push_extracted` collapses
//! dir-onto-dir so there's no stack to back out of), and `..` walks up.
//!
//! The flat shape means no tree connectors and no sticky breadcrumb (every
//! row is parentless), so the generic listing engine's sticky logic simply
//! never fires. Columns (perms / size / mtime / name) are painted here with
//! the same `row::` primitives the tree source uses, so the two stay
//! aligned by construction.

use std::time::SystemTime;

use crate::theme::PeekTheme;
use crate::viewer::listing::row::{self, SizeCell};
use crate::viewer::listing::{ListSource, NameCell, RowCells};
use crate::viewer::modes::{ExtractTarget, RenderCtx};

use super::read::{DirEntry, DirEntryKind};

/// Synthetic name for the parent-directory row. Selecting it descends to
/// `Path::canonicalize(parent).parent()`, so the user can walk back up the
/// tree without a stack of frames.
pub const PARENT_LINK_NAME: &str = "..";

pub struct DirListSource {
    entries: Vec<DirEntry>,
    /// Widest mtime cell across all rows, precomputed for stable column
    /// alignment (see [`crate::viewer::listing::TreeListSource`]).
    mtime_width: usize,
}

impl DirListSource {
    /// `show_parent_link` prepends a synthetic `..` row when the canonical
    /// path has a parent. The caller computes that once at compose time so
    /// the source doesn't canonicalize on every rebuild.
    pub fn new(entries: Vec<DirEntry>, show_parent_link: bool) -> Self {
        let mut all = Vec::with_capacity(entries.len() + show_parent_link as usize);
        if show_parent_link {
            all.push(parent_link_entry());
        }
        all.extend(entries);
        let mtime_width = row::mtime_column_width(all.iter().map(|e| format_mtime(e.mtime, false)));
        Self {
            entries: all,
            mtime_width,
        }
    }
}

impl ListSource for DirListSource {
    fn len(&self) -> usize {
        self.entries.len()
    }

    /// Flat listing — no row has a parent, so the sticky breadcrumb stays
    /// empty.
    fn parent(&self, _idx: usize) -> Option<usize> {
        None
    }

    /// Every entry is selectable (files, dirs, and `..` alike).
    fn selectable(&self, _idx: usize) -> bool {
        true
    }

    fn name(&self, idx: usize) -> &str {
        &self.entries[idx].name
    }

    fn source_label(&self) -> &str {
        "directory"
    }

    fn extract_target(&self, idx: usize) -> Option<ExtractTarget> {
        Some(ExtractTarget::EntryPath(self.entries[idx].name.clone()))
    }

    fn row_cells(&self, idx: usize, ctx: &RenderCtx) -> RowCells {
        let entry = &self.entries[idx];
        let perms = format_perms(entry);
        let size = format_size(entry);
        let is_dir = entry.kind == DirEntryKind::Dir;
        let left = row::file_row_left(
            &perms,
            &size,
            entry.size,
            is_dir,
            self.mtime_width,
            ctx.term_cols,
            ctx.peek_theme,
            || format_mtime(entry.mtime, ctx.render_opts.utc),
        );
        RowCells {
            prefix: String::new(),
            left,
            name: NameCell {
                text: entry.name.clone(),
                is_dir,
            },
        }
    }

    fn flat_line(&self, idx: usize, theme: &PeekTheme) -> Option<String> {
        // `--list` flat view: one name per line for easy piping. Dirs are
        // included (with a `/` suffix), name painted foreground throughout.
        let entry = &self.entries[idx];
        let is_dir = entry.kind == DirEntryKind::Dir;
        let perms = format_perms(entry);
        let size = format_size(entry);
        let suffix = if is_dir { "/" } else { "" };
        let painted_name = theme.paint(&format!("{}{}", entry.name, suffix), theme.foreground);
        Some(row::compose_row(
            &row::paint_perms(&perms, theme),
            &row::paint_size(&size, entry.size, is_dir, theme),
            None,
            &painted_name,
        ))
    }
}

fn format_perms(entry: &DirEntry) -> String {
    let type_ch = match (entry.is_symlink, entry.kind) {
        (true, _) => 'l',
        (false, DirEntryKind::Dir) => 'd',
        (false, DirEntryKind::File) => '-',
        (false, DirEntryKind::Other) => '?',
    };
    row::format_perms(type_ch, entry.mode, entry.kind == DirEntryKind::Dir)
}

fn format_size(entry: &DirEntry) -> String {
    let cell = if entry.kind == DirEntryKind::Dir {
        SizeCell::Dir
    } else if entry.stat_error {
        SizeCell::Unknown
    } else {
        SizeCell::Bytes(entry.size)
    };
    row::format_size(cell)
}

fn parent_link_entry() -> DirEntry {
    DirEntry {
        name: PARENT_LINK_NAME.to_string(),
        kind: DirEntryKind::Dir,
        size: 0,
        mtime: None,
        mode: None,
        is_symlink: false,
        stat_error: false,
    }
}

fn format_mtime(mtime: Option<SystemTime>, utc: bool) -> String {
    let Some(t) = mtime else {
        return "-".to_string();
    };
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => row::format_mtime_epoch(d.as_secs(), utc),
        Err(_) => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> DirEntry {
        DirEntry {
            name: name.to_string(),
            kind: DirEntryKind::File,
            size: 0,
            mtime: None,
            mode: None,
            is_symlink: false,
            stat_error: false,
        }
    }

    fn dir(name: &str) -> DirEntry {
        DirEntry {
            kind: DirEntryKind::Dir,
            ..file(name)
        }
    }

    #[test]
    fn every_row_selectable_and_flat() {
        let src = DirListSource::new(vec![dir("src"), file("main.rs")], false);
        assert_eq!(src.len(), 2);
        assert!((0..src.len()).all(|i| src.selectable(i)));
        assert!((0..src.len()).all(|i| src.parent(i).is_none()));
    }

    #[test]
    fn parent_link_prepended_when_requested() {
        let with = DirListSource::new(vec![file("main.rs")], true);
        assert_eq!(with.name(0), PARENT_LINK_NAME);
        assert_eq!(with.len(), 2);
        let without = DirListSource::new(vec![file("main.rs")], false);
        assert_eq!(without.name(0), "main.rs");
    }

    #[test]
    fn extract_target_is_entry_name() {
        let src = DirListSource::new(vec![file("main.rs")], false);
        match src.extract_target(0) {
            Some(ExtractTarget::EntryPath(p)) => assert_eq!(p, "main.rs"),
            other => panic!("expected EntryPath, got {other:?}"),
        }
    }

    #[test]
    fn symlink_renders_l_type_char() {
        let mut e = file("link");
        e.is_symlink = true;
        assert!(format_perms(&e).starts_with('l'));
    }
}
