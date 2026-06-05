//! One-level filesystem read. Returns entries sorted dirs-first,
//! case-insensitive by name. Hidden entries (dotfiles) are included —
//! peek shows everything by default; users can pipe through other tools
//! if they want to filter.

use std::cmp::Ordering;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};

#[derive(Clone)]
pub struct DirEntry {
    pub name: String,
    pub kind: DirEntryKind,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    pub mode: Option<u32>,
    /// `true` when the entry is a symlink, regardless of what it
    /// resolves to. Used as a hint in the listing.
    pub is_symlink: bool,
    /// `true` when stat-ing the entry itself failed (broken symlink,
    /// permission denied). Size / mtime / mode then carry zero / `None`
    /// values and the row renders without them.
    pub stat_error: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DirEntryKind {
    File,
    Dir,
    /// Anything that isn't a regular file or directory after symlink
    /// resolution (broken symlink, socket, FIFO, device node).
    Other,
}

/// Read a single level of `path`. Entries are sorted dirs first, then
/// by case-insensitive name. Errors at the `read_dir` boundary surface;
/// per-entry stat failures are recorded on the entry rather than
/// aborting the listing.
pub fn read_dir_entries(path: &Path) -> Result<Vec<DirEntry>> {
    let iter = fs::read_dir(path)
        .with_context(|| format!("failed to read directory {}", path.display()))?;
    let mut out = Vec::new();
    for ent in iter {
        let Ok(ent) = ent else { continue };
        let name = ent.file_name().to_string_lossy().into_owned();
        let entry_path = ent.path();
        let symlink_meta = fs::symlink_metadata(&entry_path).ok();
        let is_symlink = symlink_meta
            .as_ref()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        // Stat through the symlink so dirs-pointed-to-by-symlinks are
        // still navigable. Broken links fall back to symlink_metadata
        // so size / mtime / mode stay meaningful.
        let resolved = fs::metadata(&entry_path).ok().or(symlink_meta);
        match resolved {
            Some(meta) => out.push(DirEntry {
                name,
                kind: classify(&meta.file_type()),
                size: meta.len(),
                mtime: meta.modified().ok(),
                mode: mode_from_meta(&meta),
                is_symlink,
                stat_error: false,
            }),
            None => out.push(DirEntry {
                name,
                kind: DirEntryKind::Other,
                size: 0,
                mtime: None,
                mode: None,
                is_symlink,
                stat_error: true,
            }),
        }
    }
    out.sort_by(compare_entries);
    Ok(out)
}

fn classify(ft: &fs::FileType) -> DirEntryKind {
    if ft.is_dir() {
        DirEntryKind::Dir
    } else if ft.is_file() {
        DirEntryKind::File
    } else {
        DirEntryKind::Other
    }
}

#[cfg(unix)]
fn mode_from_meta(meta: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(meta.permissions().mode())
}

#[cfg(not(unix))]
fn mode_from_meta(_meta: &fs::Metadata) -> Option<u32> {
    None
}

fn compare_entries(a: &DirEntry, b: &DirEntry) -> Ordering {
    let kind_rank = |k: DirEntryKind| match k {
        DirEntryKind::Dir => 0,
        DirEntryKind::File => 1,
        DirEntryKind::Other => 2,
    };
    kind_rank(a.kind)
        .cmp(&kind_rank(b.kind))
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}
