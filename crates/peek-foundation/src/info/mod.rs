use std::fs;
use std::time::SystemTime;

use crate::input::mime::MimeInfo;
use crate::theme::PeekTheme;

mod json;
mod render;
mod rows;
mod section;
mod time;
mod value;

pub use json::to_json;
/// `#[derive(InfoView)]` — the print-tree generator. Shares the trait's name
/// (macro vs. type namespace) the way serde's `Serialize` does.
pub use peek_foundation_derive::InfoView;
pub use render::{RenderOptions, render, thousands_sep};
pub use render::{format_size_human, paint_count, push_field, push_section_header};
pub use rows::{InfoRow, push_rows, rows_to_json};
pub use section::{InfoNode, InfoValue, InfoView, MaybeZero, render_info};
pub use time::format_archive_mtime_zoned;
pub use value::{Accent, Muted, Role, Value, Warn};

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
    pub extras: Extras,
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

impl CompressionInfo {
    /// Compression ratio `decompressed / compressed` (a 4:1 gzip → `4.0`).
    /// Zero-guarded so a missing/zero compressed size yields `0.0` rather
    /// than NaN/inf. Both the print and JSON paths round this to one
    /// decimal so the two `--info` outputs agree.
    pub fn ratio(&self) -> f64 {
        if self.compressed_size == 0 {
            0.0
        } else {
            self.decompressed_size as f64 / self.compressed_size as f64
        }
    }
}

/// Type-specific metadata, rendered into the Info view's lower section.
///
/// Each file type owns one stats struct under `types/<x>/info.rs` and
/// implements this trait there (normally via [`impl_info_extras!`]).
/// `gather` returns a boxed `dyn InfoExtras` so the info layer never has
/// to name the concrete per-type structs — the dispatch is dynamic, the
/// same shape as the [`crate::viewer::modes::Mode`] trait. This keeps the
/// per-type modules a leaf: they depend on this trait, not the reverse.
///
/// The `Any` supertrait exists only so the tests can downcast a gathered
/// payload back to its concrete struct (asserting parsed fields directly
/// is more precise than asserting rendered strings). Production never
/// downcasts — the bound is test-driven, not part of the domain.
pub trait InfoExtras: std::any::Any {
    /// Append this type's Info-view section to `lines`.
    fn render_section(&self, lines: &mut Vec<String>, theme: &PeekTheme);

    /// Structured form of this type's section for `--info --json`, as
    /// `(type_key, value)` — the JSON object nested under `type_key`
    /// (e.g. `("archive", { "entry_count": 12, … })`).
    ///
    /// Default `None`: the type hasn't been given a typed encoder yet, so
    /// the JSON path falls back to surfacing the rendered section as a
    /// `details` text array. Implemented for converted types via the
    /// three-argument form of [`impl_info_extras!`]. See `info/json.rs`.
    fn json_section(&self) -> Option<(&'static str, serde_json::Value)> {
        None
    }
}

/// Boxed per-type info payload carried by [`FileInfo::extras`].
pub type Extras = Box<dyn InfoExtras>;

/// No-op [`InfoExtras`] for synthetic `FileInfo` fixtures in tests.
/// Lets viewer-mode unit tests build a `RenderCtx` without running the
/// full `gather` hub (or naming any concrete per-type stats struct), so
/// those tests stay pure mechanics — no dependency on the reader crate.
#[cfg(any(test, feature = "testing"))]
pub struct NoExtras;

#[cfg(any(test, feature = "testing"))]
impl InfoExtras for NoExtras {
    fn render_section(&self, _lines: &mut Vec<String>, _theme: &PeekTheme) {}
}

/// Recover the concrete stats struct from an [`Extras`] payload, panicking
/// if it isn't a `T`. Upcasts the trait object to `dyn Any` (stable trait
/// upcasting, Rust ≥ 1.86). Test-only — see the `Any` note on [`InfoExtras`].
#[cfg(any(test, feature = "testing"))]
pub fn downcast_extras<T: 'static>(extras: &Extras) -> &T {
    (extras.as_ref() as &dyn std::any::Any)
        .downcast_ref::<T>()
        .expect("unexpected extras type")
}

/// Implement [`InfoExtras`] for a stats struct by delegating its section
/// render to a free function with the signature
/// `render_section(&mut Vec<String>, &Self, &PeekTheme)` — the `$render`
/// path. The struct and the function need not share a module.
#[macro_export]
macro_rules! impl_info_extras {
    // Derived form: the type derives both `serde::Serialize` and
    // `#[derive(InfoView)]`, so one view struct drives both outputs —
    // print via [`render_info`], JSON via `serde_json::to_value` nested under
    // `$key`. The preferred wiring for migrated sections.
    ($ty:ty, json = $key:literal) => {
        impl $crate::info::InfoExtras for $ty {
            fn render_section(
                &self,
                lines: &mut ::std::vec::Vec<::std::string::String>,
                theme: &$crate::theme::PeekTheme,
            ) {
                $crate::info::render_info(lines, self, theme);
            }

            fn json_section(&self) -> ::std::option::Option<(&'static str, ::serde_json::Value)> {
                ::std::option::Option::Some((
                    $key,
                    ::serde_json::to_value(self)
                        .expect(::std::concat!($key, " info view serializes")),
                ))
            }
        }
    };
    ($ty:ty, $render:path) => {
        impl $crate::info::InfoExtras for $ty {
            fn render_section(
                &self,
                lines: &mut ::std::vec::Vec<::std::string::String>,
                theme: &$crate::theme::PeekTheme,
            ) {
                $render(lines, self, theme);
            }
        }
    };
    // Three-argument form: additionally wire a typed `--info --json`
    // encoder, a free function `(&Self) -> (&'static str, serde_json::Value)`.
    ($ty:ty, $render:path, $json:path) => {
        impl $crate::info::InfoExtras for $ty {
            fn render_section(
                &self,
                lines: &mut ::std::vec::Vec<::std::string::String>,
                theme: &$crate::theme::PeekTheme,
            ) {
                $render(lines, self, theme);
            }

            fn json_section(&self) -> ::std::option::Option<(&'static str, ::serde_json::Value)> {
                ::std::option::Option::Some($json(self))
            }
        }
    };
}

#[cfg(unix)]
pub fn format_permissions_from_meta(meta: &fs::Metadata) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    Some(format_unix_permissions(
        unix_type_char(&meta.file_type()),
        mode,
    ))
}

#[cfg(not(unix))]
pub fn format_permissions_from_meta(meta: &fs::Metadata) -> Option<String> {
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
