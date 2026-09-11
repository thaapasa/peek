//! 7-Zip TOC listing via `sevenz-rust2`. Reads the archive header (file
//! list) without decompressing any payloads.

use anyhow::{Context, Result};
use sevenz_rust2::{ArchiveReader, Password};

use super::CappedList;
use crate::types::archive::reader::ReadSeek;
use crate::viewer::listing::{EntryMtime, FlatEntry};

/// Windows file-attribute bit for read-only files.
const FILE_ATTRIBUTE_READONLY: u32 = 0x0000_0001;

pub(crate) fn list(reader: Box<dyn ReadSeek>) -> Result<CappedList> {
    let archive_reader =
        ArchiveReader::new(reader, Password::empty()).context("failed to read 7z archive")?;
    let archive = archive_reader.archive();
    let mut out = CappedList::with_hint(archive.files.len());
    for entry in &archive.files {
        let path = normalize(entry.name());
        let is_dir = entry.is_directory();
        let mtime = if entry.has_last_modified_date {
            Some(EntryMtime::Utc(entry.last_modified_date().into()))
        } else {
            None
        };
        // 7z stores Windows attributes, not unix mode; see `synthesized_mode`.
        let readonly = entry.windows_attributes() & FILE_ATTRIBUTE_READONLY != 0;
        let mode = Some(crate::info::synthesized_mode(is_dir, false, readonly));
        if !out.push(FlatEntry {
            path,
            size: entry.size(),
            mtime,
            mode,
            is_dir,
        }) {
            break;
        }
    }
    Ok(out)
}

/// Normalize 7z names to forward-slash paths. 7z stores names with
/// backslashes on Windows-authored archives; convert so the tree
/// builder splits them the same way it does for tar / zip.
fn normalize(name: &str) -> String {
    name.replace('\\', "/")
}
