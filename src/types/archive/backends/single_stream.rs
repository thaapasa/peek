//! Bare single-stream compressed files (`.gz`, `.bz2`, `.xz`,
//! `.zst`) presented as one-entry archives. The "entry" is the
//! decompressed content; its name is the source filename with the
//! compression suffix stripped, so listing a `notes.txt.gz` shows
//! a single row `notes.txt` and recursive peek descends into the
//! decompressed text.
//!
//! Listing is cheap — it doesn't decompress. Size lands as `0`
//! because none of the codecs surface the uncompressed length
//! cheaply (gzip's ISIZE is mod 2^32 and unreliable on multi-GB
//! streams). The decompress runs lazily on extract / descend.

use crate::input::detect::ArchiveFormat;
use crate::types::listing::FlatEntry;

/// Best-effort name for the decompressed entry. Strips the
/// compression suffix from the source filename when present;
/// otherwise appends `-decompressed` so the entry isn't a
/// duplicate of the container name.
pub(crate) fn entry_name(source_name: &str, fmt: ArchiveFormat) -> String {
    let suffixes: &[&str] = match fmt {
        ArchiveFormat::Gz => &[".gz"],
        ArchiveFormat::Bz2 => &[".bz2"],
        ArchiveFormat::Xz => &[".xz"],
        ArchiveFormat::Zst => &[".zst"],
        _ => return source_name.to_string(),
    };
    let lower = source_name.to_ascii_lowercase();
    for suffix in suffixes {
        if lower.ends_with(suffix) {
            return source_name[..source_name.len() - suffix.len()].to_string();
        }
    }
    if source_name == "<stdin>" {
        return "decompressed".to_string();
    }
    format!("{source_name}-decompressed")
}

/// One-entry listing. The decompress doesn't run here — the entry
/// is a pointer into the wrapped stream that the extract path
/// resolves to actual bytes.
pub(crate) fn list(source_name: &str, fmt: ArchiveFormat) -> Vec<FlatEntry> {
    vec![FlatEntry {
        path: entry_name(source_name, fmt),
        size: 0,
        mtime: None,
        mode: None,
        is_dir: false,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_compression_suffix() {
        assert_eq!(entry_name("notes.txt.gz", ArchiveFormat::Gz), "notes.txt");
        assert_eq!(
            entry_name("backup.tar.bz2", ArchiveFormat::Bz2),
            "backup.tar"
        );
        assert_eq!(entry_name("LOG.XZ", ArchiveFormat::Xz), "LOG");
    }

    #[test]
    fn fallback_for_no_extension() {
        assert_eq!(
            entry_name("anonymous", ArchiveFormat::Gz),
            "anonymous-decompressed"
        );
    }

    #[test]
    fn stdin_name_collapses_to_decompressed() {
        assert_eq!(entry_name("<stdin>", ArchiveFormat::Gz), "decompressed");
    }
}
