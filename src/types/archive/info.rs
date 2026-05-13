//! Archive info-view extras: gather TOC stats, render the Archive
//! section. On listing failure the format name is preserved and the
//! error is surfaced as a warning row.

use super::reader::list_entries;
use crate::info::{FileExtras, paint_count, push_field, push_section_header, thousands_sep};
use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;
use crate::theme::PeekTheme;
use crate::viewer::listing::Stats;

pub struct ArchiveStats {
    pub format_name: &'static str,
    pub entry_count: usize,
    pub file_count: usize,
    pub dir_count: usize,
    pub total_uncompressed_size: u64,
    /// Set when listing failed (e.g. corrupt archive). When present,
    /// the info view shows this in place of stats.
    pub error: Option<String>,
}

pub fn gather_extras(source: &InputSource, format: ArchiveFormat) -> FileExtras {
    match list_entries(source, format) {
        Ok(entries) => {
            let stats = Stats::from_root(format.label(), &entries);
            FileExtras::Archive(ArchiveStats {
                format_name: stats.format_name,
                entry_count: stats.entry_count,
                file_count: stats.file_count,
                dir_count: stats.dir_count,
                total_uncompressed_size: stats.total_size,
                error: None,
            })
        }
        Err(e) => FileExtras::Archive(ArchiveStats {
            format_name: format.label(),
            entry_count: 0,
            file_count: 0,
            dir_count: 0,
            total_uncompressed_size: 0,
            error: Some(format!("{e:#}")),
        }),
    }
}

pub fn render_section(lines: &mut Vec<String>, stats: &ArchiveStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Archive", theme);
    push_field(
        lines,
        "Format",
        &theme.paint_value(stats.format_name),
        theme,
    );

    if let Some(err) = &stats.error {
        push_field(lines, "Status", &theme.paint_warning(err), theme);
        return;
    }

    push_field(
        lines,
        "Entries",
        &paint_count(stats.entry_count, theme),
        theme,
    );
    push_field(lines, "Files", &paint_count(stats.file_count, theme), theme);
    push_field(
        lines,
        "Directories",
        &paint_count(stats.dir_count, theme),
        theme,
    );
    push_field(
        lines,
        "Total size",
        &theme.paint_value(&format!(
            "{} bytes",
            thousands_sep(stats.total_uncompressed_size)
        )),
        theme,
    );
}
