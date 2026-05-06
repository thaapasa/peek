//! Archive info-view extras: gather TOC stats, render the Archive
//! section. On listing failure the format name is preserved and the
//! error is surfaced as a warning row.

use super::reader::{ArchiveStats, list_entries};
use crate::info::{FileExtras, paint_count, push_field, push_section_header, thousands_sep};
use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;
use crate::theme::PeekTheme;

pub fn gather_extras(source: &InputSource, format: ArchiveFormat) -> FileExtras {
    match list_entries(source, format) {
        Ok(entries) => {
            let stats = ArchiveStats::from_entries(format, &entries);
            FileExtras::Archive {
                format_name: stats.format_name,
                entry_count: stats.entry_count,
                file_count: stats.file_count,
                dir_count: stats.dir_count,
                total_uncompressed_size: stats.total_uncompressed_size,
                error: None,
            }
        }
        Err(e) => FileExtras::Archive {
            format_name: format.label(),
            entry_count: 0,
            file_count: 0,
            dir_count: 0,
            total_uncompressed_size: 0,
            error: Some(format!("{e:#}")),
        },
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_section(
    lines: &mut Vec<String>,
    format_name: &str,
    entry_count: usize,
    file_count: usize,
    dir_count: usize,
    total_uncompressed_size: u64,
    error: Option<&str>,
    theme: &PeekTheme,
) {
    lines.push(String::new());
    push_section_header(lines, "Archive", theme);
    push_field(lines, "Format", &theme.paint_value(format_name), theme);

    if let Some(err) = error {
        push_field(lines, "Status", &theme.paint_warning(err), theme);
        return;
    }

    push_field(lines, "Entries", &paint_count(entry_count, theme), theme);
    push_field(lines, "Files", &paint_count(file_count, theme), theme);
    push_field(lines, "Directories", &paint_count(dir_count, theme), theme);
    push_field(
        lines,
        "Total size",
        &theme.paint_value(&format!("{} bytes", thousands_sep(total_uncompressed_size))),
        theme,
    );
}
