//! Listing table-of-contents view: tree-style hierarchical listing
//! with permissions, size, mtime, and name. Generic over the source —
//! used by archives, ISO 9660 disk images, and any future container
//! type that produces a [`super::entry::Entry`] tree.
//!
//! Listing-only: no payload extraction. The mode owns the tree, a
//! pre-flattened row index for O(1) scrolling, and the format label.

use anyhow::Result;
use syntect::highlighting::Color;

use super::entry::{Entry, EntryKind, EntryMtime};
use crate::info::RenderOptions;
use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::{PeekTheme, lerp_color};
use crate::viewer::modes::{Handled, Mode, ModeId, Position, RenderCtx, Window};
use crate::viewer::ui::Action;

/// Width (chars) of the size column, including thousands separators.
const SIZE_COL_WIDTH: usize = 12;
/// Width (chars) of the permissions column. 10-char `drwxr-xr-x` form.
const PERMS_COL_WIDTH: usize = 10;
/// Below this terminal width the mtime column is dropped to leave room
/// for the path. The cutoff matches `perms + size + path-headroom +
/// gutters` ≈ what fits comfortably without mtime.
const MTIME_HIDE_BELOW_COLS: usize = 80;

pub struct ListingMode {
    format_name: String,
    label: String,
    /// Pre-flattened tree-walk rows. Populated once at construction;
    /// scrolling slices into this without rebuilding.
    rows: Vec<TreeRow>,
    pending_warnings: Vec<String>,
    top_index: usize,
    cached_rows: usize,
    /// When true, the ancestor chain of the top visible row is pinned
    /// to the upper rows of the viewport so deeply-scrolled trees keep
    /// their breadcrumb visible. Toggled by `s`. Suppressed
    /// automatically when the top row has no parent (i.e. scroll is at
    /// a top-level entry) or when there's no scroll at all.
    sticky_enabled: bool,
}

/// One rendered row in the TOC. Holds enough metadata to render
/// without traversing the source tree again.
#[derive(Clone)]
struct TreeRow {
    /// Composed tree prefix: ancestor segments (`│ ` / `  `) plus this
    /// row's `├╴` / `└╴` connector. Empty for top-level rows.
    prefix: String,
    /// Last path segment shown alone — the tree prefix conveys depth.
    leaf: String,
    is_dir: bool,
    size: u64,
    mode: Option<u32>,
    mtime: Option<EntryMtime>,
    /// Index of the row representing this entry's parent directory in
    /// `ListingMode::rows`, or `None` for top-level entries. Used to
    /// build the sticky breadcrumb chain on scroll.
    parent_row: Option<usize>,
}

impl ListingMode {
    pub fn new(
        format_name: impl Into<String>,
        label: impl Into<String>,
        entries: Vec<Entry>,
        warnings: Vec<String>,
    ) -> Self {
        let rows = flatten(&entries);
        Self {
            format_name: format_name.into(),
            label: label.into(),
            rows,
            pending_warnings: warnings,
            top_index: 0,
            // Set on first render_window / on_resize. `scroll` and
            // `status_segments` guard with `.max(1)` for the brief
            // window before the first render.
            cached_rows: 0,
            sticky_enabled: true,
        }
    }

    /// Ancestor row indices for the row currently at the top of the
    /// viewport, ordered root-most first. Empty when sticky is off, when
    /// there's no scroll, or when the top row is a top-level entry.
    /// Capped to `(viewport / 3).max(1)` so sticky never eats more than
    /// a third of the visible content.
    fn sticky_chain(&self, viewport: usize) -> Vec<usize> {
        if !self.sticky_enabled || self.top_index == 0 || self.rows.is_empty() {
            return Vec::new();
        }
        let cap = (viewport / 3).max(1);
        let mut chain = Vec::new();
        let mut cur = self.rows[self.top_index].parent_row;
        while let Some(p) = cur {
            chain.push(p);
            cur = self.rows[p].parent_row;
        }
        chain.reverse();
        if chain.len() > cap {
            let drop = chain.len() - cap;
            chain.drain(..drop);
        }
        chain
    }

    fn max_top(&self) -> usize {
        self.rows.len().saturating_sub(self.cached_rows.max(1))
    }

    fn paint_row(
        &self,
        row: &TreeRow,
        theme: &PeekTheme,
        mtime_text: Option<(&str, usize)>,
    ) -> String {
        let perms = format_perms(row.mode, row.is_dir);
        let size = format_size(row.size, row.is_dir);
        let painted_perms = paint_perms(&perms, theme);
        let painted_size = paint_size(&size, row.size, row.is_dir, theme);
        let painted_path = paint_tree_path(&row.prefix, &row.leaf, row.is_dir, theme);
        match mtime_text {
            Some((text, width)) => {
                let padded = format!("{text:<width$}");
                let painted_mtime = theme.paint(&padded, theme.muted);
                format!("{painted_perms}  {painted_size}  {painted_mtime}  {painted_path}")
            }
            None => {
                format!("{painted_perms}  {painted_size}  {painted_path}")
            }
        }
    }

    /// Render every visible row, padding the mtime column to the widest
    /// formatted string in the slice so the path column always abuts the
    /// mtime gutter without trailing whitespace.
    fn render_slice(
        &self,
        slice: &[TreeRow],
        theme: &PeekTheme,
        opts: RenderOptions,
        show_mtime: bool,
    ) -> Vec<String> {
        if !show_mtime {
            return slice
                .iter()
                .map(|r| self.paint_row(r, theme, None))
                .collect();
        }
        let mtimes: Vec<String> = slice
            .iter()
            .map(|r| format_mtime(r.mtime.as_ref(), opts.utc))
            .collect();
        let width = mtimes.iter().map(|s| s.len()).max().unwrap_or(0);
        slice
            .iter()
            .zip(mtimes.iter())
            .map(|(r, m)| self.paint_row(r, theme, Some((m, width))))
            .collect()
    }
}

impl Mode for ListingMode {
    fn id(&self) -> ModeId {
        ModeId::Listing
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, rows: usize) -> Result<Window> {
        self.cached_rows = rows;
        let max = self.max_top();
        if self.top_index > max {
            self.top_index = max;
        }
        let show_mtime = ctx.term_cols >= MTIME_HIDE_BELOW_COLS;
        let sticky = self.sticky_chain(rows);
        let content_rows = rows.saturating_sub(sticky.len()).max(1);
        let end = self
            .top_index
            .saturating_add(content_rows)
            .min(self.rows.len());
        // Compose sticky breadcrumb rows + content slice into one
        // buffer so render_slice computes mtime column width across
        // the full visible window — keeps columns aligned.
        let mut combined: Vec<TreeRow> = Vec::with_capacity(sticky.len() + (end - self.top_index));
        for idx in &sticky {
            combined.push(self.rows[*idx].clone());
        }
        combined.extend(self.rows[self.top_index..end].iter().cloned());
        let lines = self.render_slice(&combined, ctx.peek_theme, ctx.render_opts, show_mtime);
        Ok(Window {
            lines,
            total: self.rows.len(),
        })
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let show_mtime = ctx.term_cols >= MTIME_HIDE_BELOW_COLS;
        for line in self.render_slice(&self.rows, ctx.peek_theme, ctx.render_opts, show_mtime) {
            out.write_line(&line)?;
        }
        Ok(())
    }

    fn total_lines(&self) -> Option<usize> {
        Some(self.rows.len())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        let max = self.max_top();
        let rows = self.cached_rows.max(1);
        let new_top = match action {
            Action::ScrollUp => self.top_index.saturating_sub(1),
            Action::ScrollDown => self.top_index.saturating_add(1).min(max),
            Action::PageUp => self.top_index.saturating_sub(rows.saturating_sub(1)),
            Action::PageDown => self
                .top_index
                .saturating_add(rows.saturating_sub(1))
                .min(max),
            Action::Top => 0,
            Action::Bottom => max,
            _ => return false,
        };
        self.top_index = new_top;
        true
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn on_resize(&mut self, _term_cols: usize, term_rows: usize) {
        self.cached_rows = term_rows;
    }

    fn tracks_position(&self) -> bool {
        true
    }

    fn position(&self) -> Position {
        Position::Line(self.top_index)
    }

    fn set_position(&mut self, pos: Position, _source: &InputSource) {
        if let Position::Line(l) = pos {
            self.top_index = l.min(self.max_top());
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let total = self.rows.len();
        let viewport = self.cached_rows.max(1);
        let mut segs = Vec::new();
        let s = if total <= viewport {
            format!("{} ({})", total, self.format_name)
        } else {
            format!("{}/{} ({})", self.top_index + 1, total, self.format_name)
        };
        segs.push((s, theme.muted));
        // Sticky on is the default — only call out the off state.
        if !self.sticky_enabled {
            segs.push(("sticky off".to_string(), theme.muted));
        }
        segs
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        const ACTIONS: &[(Action, &str)] = &[(Action::ToggleStickyParents, "Pin parent path")];
        ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        if action == Action::ToggleStickyParents {
            self.sticky_enabled = !self.sticky_enabled;
            return Handled::Yes;
        }
        Handled::No
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }
}

fn flatten(entries: &[Entry]) -> Vec<TreeRow> {
    // Top level: render flush-left without tree connectors. Every
    // depth-1 row would otherwise carry the same `├╴` / `└╴` at
    // column 0, which is visual noise without payload.
    let mut rows = Vec::new();
    for entry in entries {
        rows.push(TreeRow {
            prefix: String::new(),
            leaf: entry.name.clone(),
            is_dir: entry.is_dir(),
            size: entry.size,
            mode: entry.mode,
            mtime: entry.mtime.clone(),
            parent_row: None,
        });
        if let EntryKind::Dir { children } = &entry.kind {
            let parent = rows.len() - 1;
            walk(children, Some(parent), "", &mut rows);
        }
    }
    rows
}

fn walk(
    entries: &[Entry],
    parent_row: Option<usize>,
    parent_prefix: &str,
    rows: &mut Vec<TreeRow>,
) {
    let count = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        let is_last = i + 1 == count;
        // 2-column connectors: corner/tee + thin half-line ("╴", U+2574)
        // that ends at the cell boundary so the leaf abuts cleanly
        // without a separator space. Continuation columns are 2 chars
        // wide as well — vertical bar + space, or two spaces under the
        // last child of a parent.
        let connector = if is_last {
            "\u{2514}\u{2574}"
        } else {
            "\u{251c}\u{2574}"
        };
        rows.push(TreeRow {
            prefix: format!("{parent_prefix}{connector}"),
            leaf: entry.name.clone(),
            is_dir: entry.is_dir(),
            size: entry.size,
            mode: entry.mode,
            mtime: entry.mtime.clone(),
            parent_row,
        });
        if let EntryKind::Dir { children } = &entry.kind {
            let cont = if is_last { "  " } else { "\u{2502} " };
            let next_prefix = format!("{parent_prefix}{cont}");
            let new_parent = rows.len() - 1;
            walk(children, Some(new_parent), &next_prefix, rows);
        }
    }
}

/// Render the 10-char `drwxr-xr-x`-style permission string. When mode
/// is unset (implicit tree parents that don't appear in the source's
/// own entry list, or sources that don't carry mode bits at all), fall
/// back to typical defaults — `rwxr-xr-x` for dirs, `rw-r--r--` for
/// files — so the column stays informative instead of dissolving into
/// a wall of `?`s.
fn format_perms(mode: Option<u32>, is_dir: bool) -> String {
    let type_ch = if is_dir { 'd' } else { '-' };
    let mode = mode.unwrap_or(if is_dir { 0o755 } else { 0o644 });
    let mut s = String::with_capacity(10);
    s.push(type_ch);
    for (r, w, x) in [
        (0o400, 0o200, 0o100),
        (0o040, 0o020, 0o010),
        (0o004, 0o002, 0o001),
    ] {
        s.push(if mode & r != 0 { 'r' } else { '-' });
        s.push(if mode & w != 0 { 'w' } else { '-' });
        s.push(if mode & x != 0 { 'x' } else { '-' });
    }
    s
}

fn format_size(size: u64, is_dir: bool) -> String {
    let raw = if is_dir {
        "-".to_string()
    } else {
        crate::info::thousands_sep(size)
    };
    format!("{raw:>w$}", w = SIZE_COL_WIDTH)
}

fn format_mtime(mtime: Option<&EntryMtime>, utc: bool) -> String {
    use std::time::SystemTime;
    let Some(mtime) = mtime else {
        return "-".to_string();
    };
    match mtime {
        EntryMtime::Utc(t) => match t.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => crate::info::format_archive_mtime_zoned(d.as_secs(), utc),
            Err(_) => "-".to_string(),
        },
        EntryMtime::LocalNaive {
            year,
            month,
            day,
            hour,
            minute,
        } => format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}"),
    }
}

fn paint_perms(perms: &str, theme: &PeekTheme) -> String {
    let mut out = String::new();
    for (i, ch) in perms.chars().enumerate() {
        let color = match ch {
            'r' => theme.value,
            'w' => theme.accent,
            'x' => theme.heading,
            'd' | 'l' => theme.heading,
            '-' => lerp_color(theme.muted, theme.background, 0.3),
            _ => theme.foreground,
        };
        out.push_str(&theme.paint(&ch.to_string(), color));
        if (i == 3 || i == 6) && i + 1 < PERMS_COL_WIDTH {
            out.push_str(&theme.paint("\u{2500}", lerp_color(theme.muted, theme.background, 0.5)));
        }
    }
    out
}

fn paint_size(text: &str, size: u64, is_dir: bool, theme: &PeekTheme) -> String {
    if is_dir || size == 0 {
        theme.paint(text, theme.muted)
    } else {
        theme.paint(text, size_color(size, theme))
    }
}

fn size_color(bytes: u64, theme: &PeekTheme) -> Color {
    let kb = bytes as f64 / 1024.0;
    if kb < 1.0 {
        lerp_color(theme.muted, theme.value, (kb as f32).max(0.2))
    } else if kb < 1024.0 {
        theme.value
    } else {
        let t = ((kb / 1024.0).ln() / 100_f64.ln()) as f32;
        lerp_color(theme.value, theme.accent, t.clamp(0.0, 1.0))
    }
}

/// Tree prefix in muted, leaf name in foreground (or accent for dirs),
/// with a trailing `/` for directory entries.
fn paint_tree_path(prefix: &str, leaf: &str, is_dir: bool, theme: &PeekTheme) -> String {
    let leaf_color = if is_dir {
        theme.accent
    } else {
        theme.foreground
    };
    let trailing = if is_dir { "/" } else { "" };
    format!(
        "{}{}{}",
        theme.paint(prefix, theme.muted),
        theme.paint(leaf, leaf_color),
        theme.paint(trailing, theme.muted),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal listing tree:
    ///   sub/                  (row 0)
    ///     deeper/             (row 1, parent=0)
    ///       deep.txt          (row 2, parent=1)
    ///     inner.txt           (row 3, parent=0)
    ///   README.txt            (row 4, parent=None)
    fn sample() -> ListingMode {
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
        ListingMode::new("test", "TOC", entries, Vec::new())
    }

    #[test]
    fn parent_row_indices_populated() {
        let lm = sample();
        let parents: Vec<Option<usize>> = lm.rows.iter().map(|r| r.parent_row).collect();
        assert_eq!(parents, vec![None, Some(0), Some(1), Some(0), None]);
    }

    #[test]
    fn sticky_chain_empty_at_top() {
        let mut lm = sample();
        lm.top_index = 0;
        assert!(lm.sticky_chain(20).is_empty());
    }

    #[test]
    fn sticky_chain_walks_ancestors_root_first() {
        let mut lm = sample();
        // Top of viewport is `deep.txt` (row 2). Ancestors: sub/ (0)
        // → deeper/ (1).
        lm.top_index = 2;
        assert_eq!(lm.sticky_chain(20), vec![0, 1]);
    }

    #[test]
    fn sticky_chain_capped_to_viewport_third() {
        let mut lm = sample();
        lm.top_index = 2;
        // Viewport 3 → cap = 1 → keep only the deepest ancestor.
        assert_eq!(lm.sticky_chain(3), vec![1]);
    }

    #[test]
    fn sticky_chain_empty_when_disabled() {
        let mut lm = sample();
        lm.top_index = 2;
        lm.sticky_enabled = false;
        assert!(lm.sticky_chain(20).is_empty());
    }

    #[test]
    fn sticky_chain_empty_for_top_level_row() {
        let mut lm = sample();
        // Row 4 is `README.txt`, a top-level entry with parent_row = None.
        lm.top_index = 4;
        assert!(lm.sticky_chain(20).is_empty());
    }
}
