//! Archive info-view extras: TOC stats only (no payload extraction).
//!
//! On listing failure (corrupt archive, unsupported variant, I/O error)
//! the format name is preserved and the error is surfaced as a warning
//! row in the info view.

use super::super::FileExtras;
use crate::archive::{self, ArchiveStats};
use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;

pub(super) fn archive_extras(source: &InputSource, format: ArchiveFormat) -> FileExtras {
    match archive::list_entries(source, format) {
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
