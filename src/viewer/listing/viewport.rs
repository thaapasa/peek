//! Scroll + selection state for [`super::mode::ListingMode`].
//!
//! Owns the invariants and exposes one canonical mutator path. Every
//! public mutator ends in [`ListingViewport::reconcile`] so callers
//! can't leave the viewport in a state where:
//!
//! - `top` exceeds `max_top` (would scroll past the tail), or
//! - `selected` points at a directory or out-of-range row, or
//! - `selected` falls outside the *content* slot — the viewport rows
//!   below the sticky breadcrumb. The pre-extraction code conflated
//!   "viewport rows" (terminal lines) with "content rows" (viewport
//!   minus sticky chain), and several call sites checked one when
//!   they meant the other.
//!
//! Sticky chain length depends on which row is at `top`, so reconcile
//! is a fix-point loop: pull `top` to make `selected` visible,
//! recompute sticky_len at the new `top`, repeat until stable. The
//! sticky cap is `viewport / 3`, so the loop converges in a handful
//! of iterations at worst.

use std::ops::Range;

use super::mode::TreeRow;

/// Visible region resolved from the current viewport state.
/// `sticky` is ancestor row indices, root-most first; `content` is the
/// row range that follows below them.
#[derive(Debug, Clone)]
pub(super) struct VisibleWindow {
    pub sticky: Vec<usize>,
    pub content: Range<usize>,
}

pub(super) struct ListingViewport {
    viewport_rows: usize,
    top: usize,
    selected: Option<usize>,
    sticky_enabled: bool,
}

impl ListingViewport {
    pub fn new(rows: &[TreeRow]) -> Self {
        Self {
            viewport_rows: 0,
            top: 0,
            selected: first_file_row(rows),
            sticky_enabled: true,
        }
    }

    pub fn top(&self) -> usize {
        self.top
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn sticky_enabled(&self) -> bool {
        self.sticky_enabled
    }

    pub fn selected_inner_path<'a>(&self, rows: &'a [TreeRow]) -> Option<&'a str> {
        self.selected
            .and_then(|i| rows.get(i).and_then(|r| r.inner_path.as_deref()))
    }

    /// 1-based file-row position of the current selection.
    pub fn selected_file_pos(&self, rows: &[TreeRow]) -> Option<usize> {
        let sel = self.selected?;
        let mut pos = 0usize;
        for (i, row) in rows.iter().enumerate() {
            if row.inner_path.is_some() {
                pos += 1;
                if i == sel {
                    return Some(pos);
                }
            }
        }
        None
    }

    /// Resolve the visible window: sticky breadcrumb above, content
    /// row range below. Used by render and also by tests as a single
    /// source of truth for "what's on screen".
    pub fn window(&self, rows: &[TreeRow]) -> VisibleWindow {
        let viewport = self.viewport_rows.max(1);
        let sticky = self.sticky_chain(rows);
        let content_rows = viewport.saturating_sub(sticky.len()).max(1);
        let end = self.top.saturating_add(content_rows).min(rows.len());
        VisibleWindow {
            sticky,
            content: self.top..end,
        }
    }

    pub fn set_viewport_rows(&mut self, rows: &[TreeRow], n: usize) {
        self.viewport_rows = n;
        self.reconcile(rows);
    }

    pub fn toggle_sticky(&mut self, rows: &[TreeRow]) {
        self.sticky_enabled = !self.sticky_enabled;
        self.reconcile(rows);
    }

    pub fn move_selection(&mut self, rows: &[TreeRow], forward: bool) {
        if let Some(cur) = self.selected
            && let Some(next) = next_file_row(rows, cur, forward)
        {
            self.selected = Some(next);
        }
        self.reconcile(rows);
    }

    /// Page-scroll: shift `top` by content-rows minus one, then snap
    /// selection to the first file in the new content slot.
    pub fn page(&mut self, rows: &[TreeRow], forward: bool) {
        let viewport = self.viewport_rows.max(1);
        let sticky_len = self.sticky_chain_len_at(rows, self.top);
        let content = viewport.saturating_sub(sticky_len).max(1);
        let step = content.saturating_sub(1).max(1);
        if forward {
            let max = self.max_top(rows);
            self.top = self.top.saturating_add(step).min(max);
        } else {
            self.top = self.top.saturating_sub(step);
        }
        if let Some(idx) = self.first_file_in_content(rows) {
            self.selected = Some(idx);
        }
        self.reconcile(rows);
    }

    pub fn jump_first(&mut self, rows: &[TreeRow]) {
        self.selected = first_file_row(rows);
        self.top = 0;
        self.reconcile(rows);
    }

    pub fn jump_last(&mut self, rows: &[TreeRow]) {
        self.selected = last_file_row(rows);
        self.top = self.max_top(rows);
        self.reconcile(rows);
    }

    /// Restore a previously saved top position (e.g. mode swap).
    /// Selection is left as-is; reconcile pulls things straight.
    pub fn set_top(&mut self, rows: &[TreeRow], top: usize) {
        self.top = top;
        self.reconcile(rows);
    }

    /// Pin selection to a specific file row (must be a file, i.e. carry
    /// an `inner_path`). Caller is responsible for that invariant —
    /// `reconcile` here only enforces visibility and top clamp.
    pub fn select_row(&mut self, rows: &[TreeRow], row_idx: usize) {
        if row_idx < rows.len() && rows[row_idx].inner_path.is_some() {
            self.selected = Some(row_idx);
        }
        self.reconcile(rows);
    }

    /// Scroll a row into view without changing the file selection.
    /// Used by listing search when the active match lands on a
    /// directory row; selection (file-only) stays put, but the matched
    /// directory still needs to be visible.
    pub fn scroll_to_row(&mut self, rows: &[TreeRow], row_idx: usize) {
        if row_idx >= rows.len() {
            return;
        }
        let viewport = self.viewport_rows.max(1);
        let sticky_len = self.sticky_chain_len_at(rows, self.top);
        let content = viewport.saturating_sub(sticky_len).max(1);
        if row_idx < self.top {
            self.top = row_idx;
        } else if row_idx >= self.top + content {
            self.top = row_idx.saturating_sub(content - 1);
        }
        let max = self.max_top(rows);
        if self.top > max {
            self.top = max;
        }
    }

    fn reconcile(&mut self, rows: &[TreeRow]) {
        if rows.is_empty() {
            self.top = 0;
            self.selected = None;
            return;
        }
        if let Some(s) = self.selected
            && (s >= rows.len() || rows[s].inner_path.is_none())
        {
            self.selected = first_file_row(rows);
        }
        let max = self.max_top(rows);
        if self.top > max {
            self.top = max;
        }
        let Some(sel) = self.selected else {
            return;
        };
        // Fix-point: each iter changes `top`, which can change
        // sticky_len, which changes content size. Bounded by sticky
        // cap (viewport / 3) plus a couple slack iterations.
        let cap = (self.viewport_rows.max(1) / 3).max(1) + 2;
        for _ in 0..cap {
            let viewport = self.viewport_rows.max(1);
            let sticky_len = self.sticky_chain_len_at(rows, self.top);
            let content = viewport.saturating_sub(sticky_len).max(1);
            if sel < self.top {
                self.top = sel;
            } else if sel >= self.top + content {
                self.top = sel.saturating_sub(content - 1);
            } else {
                break;
            }
        }
        let max = self.max_top(rows);
        if self.top > max {
            self.top = max;
        }
    }

    /// Ancestor chain of the current `top` row, root-most first.
    /// Suppressed when sticky is off, scroll is at row 0, or the top
    /// row has no parent. Capped to `viewport / 3`.
    fn sticky_chain(&self, rows: &[TreeRow]) -> Vec<usize> {
        if !self.sticky_enabled || self.top == 0 || rows.is_empty() {
            return Vec::new();
        }
        let cap = (self.viewport_rows.max(1) / 3).max(1);
        let mut chain = Vec::new();
        let mut cur = rows[self.top].parent_row;
        while let Some(p) = cur {
            chain.push(p);
            cur = rows[p].parent_row;
        }
        chain.reverse();
        if chain.len() > cap {
            chain.drain(..chain.len() - cap);
        }
        chain
    }

    /// Length-only variant of `sticky_chain` that doesn't allocate.
    /// Used inside the reconcile / max_top fix-point loops.
    fn sticky_chain_len_at(&self, rows: &[TreeRow], top: usize) -> usize {
        if !self.sticky_enabled || top == 0 || rows.is_empty() {
            return 0;
        }
        let cap = (self.viewport_rows.max(1) / 3).max(1);
        let mut len = 0usize;
        let mut cur = rows[top].parent_row;
        while let Some(p) = cur {
            len += 1;
            cur = rows[p].parent_row;
        }
        len.min(cap)
    }

    /// Largest valid `top`. Sticky reduces the content slot below the
    /// naive `total - viewport`, so iterate forward until the tail
    /// fits inside `top..top + content_rows`.
    fn max_top(&self, rows: &[TreeRow]) -> usize {
        let viewport = self.viewport_rows.max(1);
        let total = rows.len();
        if total <= viewport {
            return 0;
        }
        let max_iter = (viewport / 3).max(1) + 1;
        let mut top = total - viewport;
        for _ in 0..max_iter {
            let sticky_len = self.sticky_chain_len_at(rows, top);
            let content = viewport.saturating_sub(sticky_len).max(1);
            if top + content >= total {
                return top;
            }
            top = (top + 1).min(total - 1);
        }
        top
    }

    fn first_file_in_content(&self, rows: &[TreeRow]) -> Option<usize> {
        (self.top..rows.len()).find(|&i| rows[i].inner_path.is_some())
    }
}

fn next_file_row(rows: &[TreeRow], from: usize, forward: bool) -> Option<usize> {
    let total = rows.len();
    if total == 0 {
        return None;
    }
    if forward {
        (from + 1..total).find(|&i| rows[i].inner_path.is_some())
    } else {
        (0..from).rev().find(|&i| rows[i].inner_path.is_some())
    }
}

fn first_file_row(rows: &[TreeRow]) -> Option<usize> {
    rows.iter().position(|r| r.inner_path.is_some())
}

fn last_file_row(rows: &[TreeRow]) -> Option<usize> {
    rows.iter().rposition(|r| r.inner_path.is_some())
}

#[cfg(test)]
mod tests {
    use super::super::entry::{Entry, EntryKind};
    use super::super::mode::flatten_for_test;
    use super::*;

    /// Same shape as the ListingMode test fixture:
    ///   sub/                  (row 0)
    ///     deeper/             (row 1, parent=0)
    ///       deep.txt          (row 2, parent=1)
    ///     inner.txt           (row 3, parent=0)
    ///   README.txt            (row 4, parent=None)
    fn sample_rows() -> Vec<TreeRow> {
        let entries = vec![
            Entry {
                name: "sub".into(),
                size: 0,
                mtime: None,
                mode: None,
                kind: EntryKind::Dir {
                    children: vec![
                        Entry {
                            name: "deeper".into(),
                            size: 0,
                            mtime: None,
                            mode: None,
                            kind: EntryKind::Dir {
                                children: vec![Entry {
                                    name: "deep.txt".into(),
                                    size: 4,
                                    mtime: None,
                                    mode: None,
                                    kind: EntryKind::File,
                                }],
                            },
                        },
                        Entry {
                            name: "inner.txt".into(),
                            size: 5,
                            mtime: None,
                            mode: None,
                            kind: EntryKind::File,
                        },
                    ],
                },
            },
            Entry {
                name: "README.txt".into(),
                size: 8,
                mtime: None,
                mode: None,
                kind: EntryKind::File,
            },
        ];
        flatten_for_test(&entries)
    }

    #[test]
    fn initial_selection_is_first_file() {
        let rows = sample_rows();
        let vp = ListingViewport::new(&rows);
        assert_eq!(vp.selected(), Some(2));
    }

    #[test]
    fn move_selection_skips_directories() {
        let rows = sample_rows();
        let mut vp = ListingViewport::new(&rows);
        vp.set_viewport_rows(&rows, 10);
        vp.move_selection(&rows, true);
        assert_eq!(vp.selected(), Some(3));
        vp.move_selection(&rows, true);
        assert_eq!(vp.selected(), Some(4));
        vp.move_selection(&rows, true);
        assert_eq!(vp.selected(), Some(4)); // sticks at end
        vp.move_selection(&rows, false);
        assert_eq!(vp.selected(), Some(3));
    }

    #[test]
    fn jump_first_last_target_file_rows() {
        let rows = sample_rows();
        let mut vp = ListingViewport::new(&rows);
        vp.set_viewport_rows(&rows, 10);
        vp.jump_last(&rows);
        assert_eq!(vp.selected(), Some(4));
        vp.jump_first(&rows);
        assert_eq!(vp.selected(), Some(2));
    }

    #[test]
    fn window_at_top_has_no_sticky_chain() {
        let rows = sample_rows();
        let mut vp = ListingViewport::new(&rows);
        vp.set_viewport_rows(&rows, 20);
        vp.set_top(&rows, 0);
        let w = vp.window(&rows);
        assert!(w.sticky.is_empty());
        assert_eq!(w.content, 0..5);
    }

    /// Regression: with sticky pinning ancestors above the content
    /// slot, scrolling selection must keep it inside the *content*
    /// slot — not just inside the full viewport. Earlier code used the
    /// full viewport size in the visibility check, so the selection
    /// could fall behind by `sticky_len` rows before scroll fired.
    #[test]
    fn selection_stays_within_content_slot_with_sticky() {
        // Build a deeper tree so sticky kicks in:
        //   a/
        //     b/
        //       c/
        //         f1.txt
        //         f2.txt
        //         f3.txt
        let entries = vec![Entry {
            name: "a".into(),
            size: 0,
            mtime: None,
            mode: None,
            kind: EntryKind::Dir {
                children: vec![Entry {
                    name: "b".into(),
                    size: 0,
                    mtime: None,
                    mode: None,
                    kind: EntryKind::Dir {
                        children: vec![Entry {
                            name: "c".into(),
                            size: 0,
                            mtime: None,
                            mode: None,
                            kind: EntryKind::Dir {
                                children: (1..=3)
                                    .map(|i| Entry {
                                        name: format!("f{i}.txt"),
                                        size: 1,
                                        mtime: None,
                                        mode: None,
                                        kind: EntryKind::File,
                                    })
                                    .collect(),
                            },
                        }],
                    },
                }],
            },
        }];
        let rows = flatten_for_test(&entries);
        // Rows: 0:a, 1:b, 2:c, 3:f1, 4:f2, 5:f3.
        let mut vp = ListingViewport::new(&rows);
        // Viewport 4 → sticky cap = 4/3 = 1. With selection visiting
        // f3.txt, sticky chain pins one ancestor → content slot = 3.
        vp.set_viewport_rows(&rows, 4);
        vp.jump_last(&rows);
        let w = vp.window(&rows);
        let sel = vp.selected().unwrap();
        assert!(
            w.content.contains(&sel),
            "selection {sel} must sit in content {:?}, sticky {:?}",
            w.content,
            w.sticky
        );
    }
}
