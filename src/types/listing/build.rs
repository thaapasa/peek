//! Tree construction from flat path-string entries.
//!
//! Most archive backends produce flat `(path, metadata)` lists — zip's
//! central directory, tar's header chain, 7z's file index. This helper
//! splits each path on `/`, walks into the tree, and synthesizes
//! intermediate directory nodes when a child references a parent the
//! source didn't list explicitly. Sources that walk natively
//! hierarchically (ISO 9660, future filesystems) build `Entry` trees
//! directly without going through this path.

use super::entry::{Entry, EntryKind, EntryMtime};

/// One flat-source entry: a slash-delimited path plus its metadata.
/// `is_dir = true` distinguishes empty directories from zero-byte
/// files (some archive formats list both).
pub struct FlatEntry {
    pub path: String,
    pub size: u64,
    pub mtime: Option<EntryMtime>,
    pub mode: Option<u32>,
    pub is_dir: bool,
}

/// Build a tree of `Entry` from flat path-keyed entries. The returned
/// vector holds top-level entries (no synthetic root node).
///
/// Implicit directories — referenced by a child path but absent from
/// the input — get default metadata (`size = 0`, `mode = None`,
/// `mtime = None`) so the renderer can fall back to typical defaults.
/// Children within each directory are sorted dirs-first then
/// alphabetically, matching `tree --dirsfirst`.
pub fn from_flat_paths(items: Vec<FlatEntry>) -> Vec<Entry> {
    let mut root: Vec<Entry> = Vec::new();
    for item in items {
        let trimmed = item.path.trim_start_matches("./").trim_end_matches('/');
        if trimmed.is_empty() {
            // The archive root entry itself (e.g. tar's leading `./`).
            // Drop — we don't render a synthetic root.
            continue;
        }
        let parts: Vec<&str> = trimmed.split('/').collect();
        insert(&mut root, &parts, &item);
    }
    sort(&mut root);
    root
}

fn insert(siblings: &mut Vec<Entry>, parts: &[&str], item: &FlatEntry) {
    let (head, tail) = parts.split_first().expect("non-empty parts");
    let pos = siblings.iter().position(|e| e.name == *head);
    if tail.is_empty() {
        // Leaf for this path. Either upgrade an existing implicit dir
        // node (already created by an earlier child reference) or add
        // a fresh entry with the source's metadata.
        if let Some(idx) = pos {
            let node = &mut siblings[idx];
            node.size = item.size;
            node.mtime = item.mtime.clone();
            node.mode = item.mode;
            // Existing tree already had children → this is a real dir.
            // If item.is_dir but no children yet, also a dir.
            // Otherwise it's a file leaf.
            if item.is_dir && !matches!(node.kind, EntryKind::Dir { .. }) {
                node.kind = EntryKind::Dir {
                    children: Vec::new(),
                };
            }
        } else {
            siblings.push(Entry {
                name: (*head).to_string(),
                size: item.size,
                mtime: item.mtime.clone(),
                mode: item.mode,
                kind: if item.is_dir {
                    EntryKind::Dir {
                        children: Vec::new(),
                    }
                } else {
                    EntryKind::File
                },
            });
        }
        return;
    }

    let dir_idx = match pos {
        Some(idx) => {
            // Promote a previously-leaf entry to a directory if we
            // discover children below it. Real-world inputs don't tend
            // to do this, but the merge keeps the tree consistent.
            let node = &mut siblings[idx];
            if !matches!(node.kind, EntryKind::Dir { .. }) {
                node.kind = EntryKind::Dir {
                    children: Vec::new(),
                };
            }
            idx
        }
        None => {
            siblings.push(Entry {
                name: (*head).to_string(),
                size: 0,
                mtime: None,
                mode: None,
                kind: EntryKind::Dir {
                    children: Vec::new(),
                },
            });
            siblings.len() - 1
        }
    };
    let EntryKind::Dir { children } = &mut siblings[dir_idx].kind else {
        unreachable!("just promoted to Dir");
    };
    insert(children, tail, item);
}

fn sort(siblings: &mut Vec<Entry>) {
    siblings.sort_by(|a, b| match (a.is_dir(), b.is_dir()) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    for e in siblings {
        if let EntryKind::Dir { children } = &mut e.kind {
            sort(children);
        }
    }
}
