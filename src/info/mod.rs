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
    pub extras: FileExtras,
}

/// Type-specific metadata.
pub enum FileExtras {
    Image {
        width: u32,
        height: u32,
        color_type: String,
        bit_depth: u8,
        hdr_format: Option<String>,
        icc_profile: Option<String>,
        animation: Option<AnimationStats>,
        exif: Vec<(String, String)>,
        xmp: Vec<(String, String)>,
    },
    Text(TextStats),
    Svg {
        text: TextStats,
        view_box: Option<String>,
        declared_width: Option<String>,
        declared_height: Option<String>,
        path_count: usize,
        group_count: usize,
        rect_count: usize,
        circle_count: usize,
        text_count: usize,
        has_script: bool,
        has_external_href: bool,
        animation: Option<SvgAnimationStats>,
        /// Set when the SVG declared an animation peek can't play
        /// (unsupported feature, malformed, or rasterization probe
        /// failed). Surfaced as a warning row in the info view.
        animation_warning: Option<String>,
    },
    Structured {
        format_name: &'static str,
        stats: Option<StructuredStats>,
    },
    Binary {
        format: Option<String>,
    },
    Archive {
        format_name: &'static str,
        entry_count: usize,
        file_count: usize,
        dir_count: usize,
        total_uncompressed_size: u64,
        /// Set when listing failed (e.g. corrupt archive). When present,
        /// the info view shows this in place of stats.
        error: Option<String>,
    },
}

/// Animation playback stats. Counts/durations may be `None` when the format
/// requires full decoding to compute (WebP) — the cheap header-walk path is
/// only available for GIF.
pub struct AnimationStats {
    pub frame_count: Option<usize>,
    pub total_duration_ms: Option<u64>,
    pub loop_count: Option<LoopCount>,
}

/// SVG CSS-keyframe animation stats (from `viewer::image::svg_anim`).
pub struct SvgAnimationStats {
    pub frame_count: usize,
    pub total_duration_ms: u64,
    pub infinite: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum LoopCount {
    Infinite,
    Finite(u32),
}

pub struct TextStats {
    pub line_count: usize,
    pub word_count: usize,
    pub char_count: usize,
    pub blank_lines: usize,
    pub longest_line_chars: usize,
    pub line_endings: LineEndings,
    pub indent_style: Option<IndentStyle>,
    pub encoding: Encoding,
    pub shebang: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LineEndings {
    None,
    Lf,
    Crlf,
    Cr,
    Mixed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Tabs,
    Spaces(u8),
    Mixed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

pub struct StructuredStats {
    pub top_level_kind: TopLevelKind,
    pub top_level_count: usize,
    pub max_depth: usize,
    pub total_nodes: usize,
    pub xml_root: Option<String>,
    pub xml_namespaces: Vec<String>,
}

pub enum TopLevelKind {
    Object,
    Array,
    Scalar,
    Table,
    MultiDoc(usize),
    Document,
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
