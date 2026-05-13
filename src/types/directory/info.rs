//! Directory info section: child counts split by kind.

use std::path::Path;

use crate::info::{FileExtras, push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;

use super::read::{DirEntryKind, read_dir_entries};

pub struct DirectoryStats {
    pub entry_count: usize,
    pub file_count: usize,
    pub dir_count: usize,
}

pub fn gather_extras(path: &Path) -> FileExtras {
    let entries = read_dir_entries(path).unwrap_or_default();
    let dir_count = entries
        .iter()
        .filter(|e| e.kind == DirEntryKind::Dir)
        .count();
    let file_count = entries
        .iter()
        .filter(|e| e.kind == DirEntryKind::File)
        .count();
    FileExtras::Directory(DirectoryStats {
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
