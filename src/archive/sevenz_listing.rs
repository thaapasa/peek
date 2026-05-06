//! 7-Zip TOC listing via `sevenz-rust2`. Reads the archive header (file
//! list) without decompressing any payloads.

use anyhow::{Context, Result};

use super::{ArchiveEntry, ArchiveMtime, ReadSeek};
use sevenz_rust2::{ArchiveReader, Password};

pub(super) fn list(reader: Box<dyn ReadSeek>) -> Result<Vec<ArchiveEntry>> {
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
        out.push(ArchiveEntry {
            path,
            size: entry.size(),
            mtime,
            // 7z carries Windows attributes, not Unix mode bits. Leaving
            // mode unset surfaces `?????????` perms, which honestly
            // describes what we know.
            mode: None,
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
