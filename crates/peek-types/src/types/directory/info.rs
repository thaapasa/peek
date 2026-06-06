//! Directory info section: child counts split by kind.

use std::path::Path;

use crate::info::{Extras, push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;

use super::read::{DirEntryKind, read_dir_entries};

pub struct DirectoryStats {
    pub entry_count: usize,
    pub file_count: usize,
    pub dir_count: usize,
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
        entry_count: entries.len(),
        file_count,
        dir_count,
    })
}

pub fn render_section(lines: &mut Vec<String>, stats: &DirectoryStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Directory", theme);
    push_field(
        lines,
        "Entries",
        &theme.paint_value(&thousands_sep(stats.entry_count as u64)),
        theme,
    );
    push_field(
        lines,
        "Files",
        &theme.paint_value(&thousands_sep(stats.file_count as u64)),
        theme,
    );
    push_field(
        lines,
        "Subdirs",
        &theme.paint_value(&thousands_sep(stats.dir_count as u64)),
        theme,
    );
}

/// Typed `--info --json` encoding of the Directory section. Counts are raw
/// JSON numbers.
pub fn json_section(stats: &DirectoryStats) -> (&'static str, serde_json::Value) {
    let obj = serde_json::json!({
        "entry_count": stats.entry_count,
        "file_count": stats.file_count,
        "dir_count": stats.dir_count,
    });
    ("directory", obj)
}
