use std::fs;
use std::time::SystemTime;

use crate::input::mime::MimeInfo;

mod gather;
mod render;
mod time;

pub use gather::gather;
pub use render::{RenderOptions, render, thousands_sep};
pub(crate) use render::{paint_count, push_field, push_section_header};
pub use time::format_archive_mtime_zoned;

// Re-exports of the per-type info shapes so the central `FileExtras`
// enum below can stay one-line-per-variant. Each type owns its own
// struct under `types/<x>/info.rs`.
pub use crate::types::archive::info::ArchiveStats;
pub use crate::types::audio::AudioStats;
pub use crate::types::binary::info::BinaryInfo;
pub use crate::types::comic::ComicStats;
pub use crate::types::directory::info::DirectoryStats;
pub use crate::types::disk_image::info::DiskImageInfo;
pub use crate::types::document::DocumentStats;
pub use crate::types::ebook::EbookStats;
pub use crate::types::image::info::ImageStats;
pub use crate::types::markdown::info::MarkdownInfo;
pub use crate::types::pdf::PdfStats;
pub use crate::types::sql::info::SqlInfo;
pub use crate::types::structured::info::StructuredInfo;
pub use crate::types::svg::info::SvgStats;
pub use crate::types::text::info::TextStats;

/// Collected file metadata.
pub struct FileInfo {
    pub file_name: String,
    pub path: String,
    pub size_bytes: u64,
    /// MIME types associated with this file, in display order. May contain
    /// the magic-byte type, the registered fallback for the format, and the
    /// extension-based convention (deduplicated).
    pub mimes: Vec<MimeInfo>,
    /// User-facing warnings (e.g. extension/MIME mismatch). Empty in the
    /// common case.
    pub warnings: Vec<String>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub permissions: Option<String>,
    /// Set when the rendered source is the inner content of a
    /// transparently-decompressed bare single-stream wrapper
    /// (`.gz` / `.bz2` / `.xz` / `.zst` / `.lz4`). Drives a
    /// Compression row in the File section.
    pub compression: Option<CompressionInfo>,
    pub extras: FileExtras,
}

/// Snapshot of a transparent decompression for the info view.
pub struct CompressionInfo {
    /// Short codec label (`gzip` / `bzip2` / `xz` / `zstd` / `lz4`).
    pub codec_label: &'static str,
    pub compressed_size: u64,
    pub decompressed_size: u64,
    /// Outer (compressed) filename so the info view can still surface
    /// it even though the visible source is the inner decompressed
    /// memory buffer.
    pub outer_name: String,
    /// Decompression error — when present, indicates the viewer is
    /// rendering the raw compressed bytes (Hex fallback) and this
    /// string explains why.
    pub error: Option<String>,
}

/// Type-specific metadata. Each variant wraps a single struct owned by
/// the corresponding `types/<x>/info.rs` module.
pub enum FileExtras {
    Image(ImageStats),
    Text(TextStats),
    Svg(SvgStats),
    Structured(StructuredInfo),
    Markdown(MarkdownInfo),
    Sql(SqlInfo),
    Binary(BinaryInfo),
    Archive(ArchiveStats),
    DiskImage(DiskImageInfo),
    Directory(DirectoryStats),
    Ebook(EbookStats),
    Comic(ComicStats),
    Document(DocumentStats),
    Pdf(PdfStats),
    Audio(AudioStats),
}

#[cfg(unix)]
pub(super) fn format_permissions_from_meta(meta: &fs::Metadata) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    Some(format_unix_permissions(
        unix_type_char(&meta.file_type()),
        mode,
    ))
}

#[cfg(not(unix))]
pub(super) fn format_permissions_from_meta(meta: &fs::Metadata) -> Option<String> {
    let perms = meta.permissions();
    Some(if perms.readonly() {
        "read-only".to_string()
    } else {
        "read-write".to_string()
    })
}

#[cfg(unix)]
fn unix_type_char(ft: &fs::FileType) -> char {
    use std::os::unix::fs::FileTypeExt;
    if ft.is_dir() {
        'd'
    } else if ft.is_symlink() {
        'l'
    } else if ft.is_block_device() {
        'b'
    } else if ft.is_char_device() {
        'c'
    } else if ft.is_fifo() {
        'p'
    } else if ft.is_socket() {
        's'
    } else {
        '-'
    }
}

#[cfg(unix)]
fn format_unix_permissions(type_char: char, mode: u32) -> String {
    let mut s = String::with_capacity(10);
    s.push(type_char);

    // Each rwx triplet's execute slot is overlaid with the matching
    // special bit (setuid for owner, setgid for group, sticky for other),
    // following `ls -l` conventions: lowercase = both bits set, uppercase
    // = only the special bit set.
    let triplets = [
        (0o400, 0o200, 0o100, 0o4000, 's'),
        (0o040, 0o020, 0o010, 0o2000, 's'),
        (0o004, 0o002, 0o001, 0o1000, 't'),
    ];
    for (r_bit, w_bit, x_bit, special_bit, special_ch) in triplets {
        s.push(if mode & r_bit != 0 { 'r' } else { '-' });
        s.push(if mode & w_bit != 0 { 'w' } else { '-' });
        let special = mode & special_bit != 0;
        let exec = mode & x_bit != 0;
        s.push(match (special, exec) {
            (true, true) => special_ch,
            (true, false) => special_ch.to_ascii_uppercase(),
            (false, true) => 'x',
            (false, false) => '-',
        });
    }
    s
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::format_unix_permissions;

    #[test]
    fn regular_file_rwx() {
        assert_eq!(format_unix_permissions('-', 0o755), "-rwxr-xr-x");
        assert_eq!(format_unix_permissions('-', 0o644), "-rw-r--r--");
        assert_eq!(format_unix_permissions('-', 0o000), "----------");
    }

    #[test]
    fn directory_prefix() {
        assert_eq!(format_unix_permissions('d', 0o755), "drwxr-xr-x");
    }

    #[test]
    fn symlink_prefix() {
        assert_eq!(format_unix_permissions('l', 0o777), "lrwxrwxrwx");
    }

    #[test]
    fn setuid_with_owner_exec() {
        // 04755: setuid + rwxr-xr-x → 's' in owner-x slot.
        assert_eq!(format_unix_permissions('-', 0o4755), "-rwsr-xr-x");
    }

    #[test]
    fn setuid_without_owner_exec() {
        // 04644: setuid + rw-r--r-- → 'S' (uppercase: special set, exec not).
        assert_eq!(format_unix_permissions('-', 0o4644), "-rwSr--r--");
    }

    #[test]
    fn setgid_with_group_exec() {
        assert_eq!(format_unix_permissions('-', 0o2755), "-rwxr-sr-x");
    }

    #[test]
    fn setgid_without_group_exec() {
        assert_eq!(format_unix_permissions('-', 0o2744), "-rwxr-Sr--");
    }

    #[test]
    fn sticky_with_other_exec() {
        // /tmp-style: drwxrwxrwt
        assert_eq!(format_unix_permissions('d', 0o1777), "drwxrwxrwt");
    }

    #[test]
    fn sticky_without_other_exec() {
        assert_eq!(format_unix_permissions('d', 0o1770), "drwxrwx--T");
    }

    #[test]
    fn all_special_bits_combined() {
        // Setuid + setgid + sticky on rwxrwxrwx.
        assert_eq!(format_unix_permissions('-', 0o7777), "-rwsrwsrwt");
    }
}
