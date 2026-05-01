use std::fs;
use std::time::SystemTime;

use crate::input::mime::MimeInfo;

mod gather;
mod render;
mod time;

pub use gather::gather;
pub use render::{render, RenderOptions};

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
        frame_count: Option<usize>,
        exif: Vec<(String, String)>,
    },
    Text {
        line_count: usize,
        word_count: usize,
        char_count: usize,
    },
    Structured {
        format_name: &'static str,
    },
    Binary,
}

#[cfg(unix)]
pub(super) fn format_permissions_from_meta(meta: &fs::Metadata) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    Some(format_unix_permissions(unix_type_char(&meta.file_type()), mode))
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
