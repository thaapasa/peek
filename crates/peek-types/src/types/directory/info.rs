//! Directory info section: child counts split by kind.

use std::path::Path;

use crate::info::{Extras, Value};

use super::read::{DirEntryKind, read_dir_entries};

/// Directory section view — drives both `--info` print and `--info --json`.
/// Counts use [`Value::Int`] (flat value colour + thousands separators), not
/// the count gradient.
#[derive(serde::Serialize, crate::info::InfoSection)]
#[info(title = "Directory")]
pub struct DirectoryStats {
    #[info(label = "Entries")]
    pub entry_count: Value,
    #[info(label = "Files")]
    pub file_count: Value,
    #[info(label = "Subdirs")]
    pub dir_count: Value,
}

pub fn gather_extras(path: &Path) -> Extras {
    let entries = read_dir_entries(path).unwrap_or_default();
    let dir_count = entries
        .iter()
        .filter(|e| e.kind == DirEntryKind::Dir)
        .count();
    let file_count = entries
        .iter()
        .filter(|e| e.kind == DirEntryKind::File)
        .count();
    Box::new(DirectoryStats {
        entry_count: Value::int(entries.len() as i64),
        file_count: Value::int(file_count as i64),
        dir_count: Value::int(dir_count as i64),
    })
}
