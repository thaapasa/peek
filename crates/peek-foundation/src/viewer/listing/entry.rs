//! Generic file-listing data shapes shared across container types
//! (archive, ISO, future epub/msi/etc).

use std::time::{Duration, SystemTime};

/// One node in the listing tree. Directories own their children; files
/// are leaves. Metadata fields are optional because some sources only
/// surface a subset (zip lacks Unix mode for non-Unix archives, ISO
/// lacks per-entry mode without Rock Ridge, implicit parent dirs
/// inferred from a child's path carry no metadata of their own).
pub struct Entry {
    /// Last path segment only — the tree position carries the rest.
    pub name: String,
    pub size: u64,
    pub mtime: Option<EntryMtime>,
    pub mode: Option<u32>,
    pub kind: EntryKind,
}

pub enum EntryKind {
    File,
    Dir { children: Vec<Entry> },
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, EntryKind::Dir { .. })
    }
}

/// Per-entry mtime. UTC for sources that timestamp in epoch seconds
/// (tar, 7z, ISO recording date with offset). LocalNaive for sources
/// that store wall-clock without a zone (zip MS-DOS dates) — keeping
/// the two distinct stops the renderer from double-applying a UTC
/// offset on naive stamps.
#[derive(Clone)]
pub enum EntryMtime {
    Utc(SystemTime),
    LocalNaive {
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
    },
}

/// Build a `SystemTime` from Unix epoch seconds, returning `None` for
/// pre-epoch or unset (`0`) timestamps.
pub fn time_from_epoch_secs(secs: u64) -> Option<SystemTime> {
    if secs == 0 {
        return None;
    }
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}
