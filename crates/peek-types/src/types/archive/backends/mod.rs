//! Format-specific TOC decoders. Each backend reads only the structural
//! metadata (central directory / header chain / file index) needed to
//! enumerate `ArchiveEntry`s — no payload extraction.

use crate::viewer::listing::FlatEntry;

pub(super) mod ar;
pub(super) mod cpio;
pub(super) mod sevenz;
pub(super) mod tar;
pub(super) mod zip;

/// Hard cap on entries enumerated from one archive listing. Bounds both the
/// in-memory `FlatEntry` metadata *and* — for streaming compressed tarballs
/// / `cpio.gz` — the decompression walk itself, since breaking the read
/// loop stops pulling from the decoder. A crafted `.tar.gz` of millions of
/// empty entries is a few MB on disk but expands to a multi-GB tar stream;
/// without this both the listing `Vec` and the decode are unbounded
/// (finding L14). 100k matches the threshold above which real archives are
/// vanishingly rare; over it the listing is truncated with a surfaced note.
pub(super) const MAX_ENTRIES: usize = 100_000;

/// Accumulates listing rows under [`MAX_ENTRIES`]. Backends push into it and
/// stop their read loop the moment [`push`](CappedList::push) returns
/// `false`, recording whether anything was dropped so the views can say so
/// instead of silently under-reporting.
#[derive(Default)]
pub(super) struct CappedList {
    pub entries: Vec<FlatEntry>,
    pub truncated: bool,
}

impl CappedList {
    /// Preallocate up to the cap — never trust an attacker-supplied entry
    /// count (a zip central directory / 7z header can claim millions).
    pub(super) fn with_hint(hint: usize) -> Self {
        Self {
            entries: Vec::with_capacity(hint.min(MAX_ENTRIES)),
            truncated: false,
        }
    }

    /// Append `entry`, or refuse and flag truncation once the cap is hit.
    /// Returns `true` while the caller may keep enumerating, `false` once
    /// full — at which point the caller must break (also halting any
    /// decompressor feeding the loop).
    pub(super) fn push(&mut self, entry: FlatEntry) -> bool {
        if self.entries.len() >= MAX_ENTRIES {
            self.truncated = true;
            return false;
        }
        self.entries.push(entry);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> FlatEntry {
        FlatEntry {
            path: "x".into(),
            size: 0,
            mtime: None,
            mode: None,
            is_dir: false,
        }
    }

    #[test]
    fn caps_at_max_and_flags_truncation() {
        let mut list = CappedList::default();
        for _ in 0..MAX_ENTRIES {
            assert!(list.push(entry()));
        }
        // Exactly at the cap: full but not truncated.
        assert!(!list.truncated);
        assert_eq!(list.entries.len(), MAX_ENTRIES);
        // One more is refused and flags truncation.
        assert!(!list.push(entry()));
        assert!(list.truncated);
        assert_eq!(list.entries.len(), MAX_ENTRIES);
    }

    #[test]
    fn with_hint_never_preallocates_past_cap() {
        let list = CappedList::with_hint(usize::MAX);
        assert!(list.entries.capacity() <= MAX_ENTRIES);
    }
}
