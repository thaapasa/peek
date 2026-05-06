//! Zip TOC listing via the `zip` crate's central-directory parser. No
//! payload decompression — only the central directory entries are read.

use anyhow::{Context, Result};

use crate::types::archive::reader::{ArchiveEntry, ArchiveMtime, ReadSeek};

pub(crate) fn list(reader: Box<dyn ReadSeek>) -> Result<Vec<ArchiveEntry>> {
    let mut archive = zip::ZipArchive::new(reader).context("failed to read zip archive")?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .with_context(|| format!("failed to read zip entry {i}"))?;
        let raw_name = file.name().to_string();
        let is_dir = file.is_dir();
        let path = normalize_path(&raw_name);
        // Zip MS-DOS dates store wall-clock without a zone — the archiver's
        // local time. Keeping it naive (rather than pretending it's UTC and
        // letting `localtime_r` add another offset) preserves the original
        // wall clock for the user's display.
        let mtime = file.last_modified().map(|dt| ArchiveMtime::LocalNaive {
            year: dt.year(),
            month: dt.month(),
            day: dt.day(),
            hour: dt.hour(),
            minute: dt.minute(),
        });
        out.push(ArchiveEntry {
            path,
            size: file.size(),
            mtime,
            mode: file.unix_mode(),
            is_dir,
        });
    }
    Ok(out)
}

/// Normalize zip names to forward-slash paths. Zip stores forward slashes
/// already; this strips a redundant leading `./` so display matches the
/// tar backend.
fn normalize_path(name: &str) -> String {
    name.strip_prefix("./").unwrap_or(name).to_string()
}
