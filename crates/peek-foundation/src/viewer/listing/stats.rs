//! Aggregate stats over a listing tree, used by the info view.

use super::entry::{Entry, EntryKind};

pub struct Stats {
    pub format_name: &'static str,
    pub entry_count: usize,
    pub file_count: usize,
    pub dir_count: usize,
    pub total_size: u64,
}

impl Stats {
    pub fn from_root(format_name: &'static str, root: &[Entry]) -> Self {
        let mut s = Self {
            format_name,
            entry_count: 0,
            file_count: 0,
            dir_count: 0,
            total_size: 0,
        };
        walk(root, &mut s);
        s
    }
}

fn walk(entries: &[Entry], s: &mut Stats) {
    for e in entries {
        s.entry_count += 1;
        match &e.kind {
            EntryKind::File => {
                s.file_count += 1;
                s.total_size = s.total_size.saturating_add(e.size);
            }
            EntryKind::Dir { children } => {
                s.dir_count += 1;
                walk(children, s);
            }
        }
    }
}
