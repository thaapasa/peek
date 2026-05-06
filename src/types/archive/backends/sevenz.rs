//! 7-Zip TOC listing via `sevenz-rust2`. Reads the archive header (file
//! list) without decompressing any payloads.

use anyhow::{Context, Result};
use sevenz_rust2::{ArchiveReader, Password};

use crate::types::archive::reader::{ArchiveEntry, ArchiveMtime, ReadSeek};

/// Windows file-attribute bit for read-only files. Used to translate the
/// 7z native attribute set into a meaningful Unix permission preview.
const FILE_ATTRIBUTE_READONLY: u32 = 0x0000_0001;

pub(crate) fn list(reader: Box<dyn ReadSeek>) -> Result<Vec<ArchiveEntry>> {
    let archive_reader =
        ArchiveReader::new(reader, Password::empty()).context("failed to read 7z archive")?;
    let archive = archive_reader.archive();
    let mut out = Vec::with_capacity(archive.files.len());
    for entry in &archive.files {
        let path = normalize(entry.name());
        let is_dir = entry.is_directory();
        let mtime = if entry.has_last_modified_date {
            Some(ArchiveMtime::Utc(entry.last_modified_date().into()))
        } else {
            None
        };
        // 7z stores Windows attributes, not Unix mode bits. Synthesize a
        // representative mode so the perms column is informative: dirs
        // get `rwxr-xr-x`, read-only files `r--r--r--`, others `rw-r--r--`.
        let attrs = entry.windows_attributes();
        let mode = Some(if is_dir {
            0o755
        } else if attrs & FILE_ATTRIBUTE_READONLY != 0 {
            0o444
        } else {
            0o644
        });
        out.push(ArchiveEntry {
            path,
            size: entry.size(),
            mtime,
            mode,
            is_dir,
        });
    }
    Ok(out)
}

/// Normalize 7z names to forward-slash paths. 7z stores names with
/// backslashes on Windows-authored archives; convert so the tree
/// builder splits them the same way it does for tar / zip.
fn normalize(name: &str) -> String {
    name.replace('\\', "/")
}
