//! Listing table-of-contents view: tree-style hierarchical listing
//! with permissions, size, mtime, and name. Generic over the source —
//! used by archives, ISO 9660 disk images, and any future container
//! type that produces a [`super::entry::Entry`] tree.
//!
//! Listing-only: no payload extraction. The mode owns the tree and
//! a pre-flattened row index; scroll + selection state lives in
//! [`super::viewport::ListingViewport`], which keeps invariants
//! (top in range, selection on a file row, selection visible inside
//! the *content* slot — not behind the sticky breadcrumb) under one
//! reconcile path so individual mode methods can't drift.

use std::ops::Range;

use anyhow::Result;
use syntect::highlighting::Color;

use super::entry::{Entry, EntryKind, EntryMtime};
use super::viewport::ListingViewport;
use crate::info::RenderOptions;
use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::{PeekTheme, lerp_color};
use crate::viewer::modes::{ExtractTarget, Handled, Mode, ModeId, Position, RenderCtx, Window};
use crate::viewer::search::{MAX_MATCHES, find_matches, overlay_matches, smart_case_sensitive};
use crate::viewer::ui::{Action, HelpEntry};

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
    viewport: ListingViewport,
    /// Active leaf-name search, if any. Matches every row (files +
    /// directories); navigation moves the file selection when the
    /// current match is on a file row, and just scrolls the row into
    /// view when it's on a directory.
    search: Option<ListingSearch>,
}

/// One leaf-name match: the row it sits on plus the byte ranges inside
/// that row's leaf string. Multi-hit leaves carry several ranges.
#[derive(Clone)]
struct LeafMatch {
    row_idx: usize,
    ranges: Vec<Range<usize>>,
}

struct ListingSearch {
    matches: Vec<LeafMatch>,
    /// Active-match index. Unused when `matches` is empty.
    cursor: usize,
}

/// One rendered row in the TOC. Holds enough metadata to render
/// without traversing the source tree again. Kept `pub(super)` so
/// the viewport module can read row metadata (parent_row,
/// inner_path) when computing scroll geometry.
#[derive(Clone)]
pub(super) struct TreeRow {
    /// Composed tree prefix: ancestor segments (`│ ` / `  `) plus this
    /// row's `├╴` / `└╴` connector. Empty for top-level rows.
    pub(super) prefix: String,
    /// Last path segment shown alone — the tree prefix conveys depth.
    pub(super) leaf: String,
    pub(super) is_dir: bool,
    pub(super) size: u64,
    pub(super) mode: Option<u32>,
    pub(super) mtime: Option<EntryMtime>,
    /// Index of the row representing this entry's parent directory in
    /// `ListingMode::rows`, or `None` for top-level entries. Used to
    /// build the sticky breadcrumb chain on scroll.
    pub(super) parent_row: Option<usize>,
    /// Slash-joined inner path for file rows; `None` for directories.
    /// Used as the extract key.
    pub(super) inner_path: Option<String>,
}

impl ListingMode {
    pub fn new(
        format_name: impl Into<String>,
        label: impl Into<String>,
        entries: Vec<Entry>,
        warnings: Vec<String>,
    ) -> Self {
        let rows = flatten(&entries);
        let viewport = ListingViewport::new(&rows);
        Self {
            format_name: format_name.into(),
            label: label.into(),
            rows,
            pending_warnings: warnings,
            viewport,
            search: None,
        }
    }

    /// File rows only, no directories.
    fn file_count(&self) -> usize {
        self.rows.iter().filter(|r| r.inner_path.is_some()).count()
    }

    fn paint_row(
        &self,
        row_idx: usize,
        row: &TreeRow,
        theme: &PeekTheme,
        mtime_text: Option<(&str, usize)>,
        selected: bool,
    ) -> String {
        let perms = format_perms(row.mode, row.is_dir);
        let size = format_size(row.size, row.is_dir);
        let painted_perms = paint_perms(&perms, theme);
        let painted_size = paint_size(&size, row.size, row.is_dir, theme);
        let (ranges, current) = self.leaf_match_ranges(row_idx);
        let painted_path = paint_tree_path(
            &row.prefix,
            &row.leaf,
            row.is_dir,
            theme,
            selected,
            &ranges,
            current,
        );
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

    /// Match ranges (in the row's leaf bytes) and which one is the
    /// active cursor, for `paint_row`. Empty when no search is active
    /// or the row carries no hits.
    fn leaf_match_ranges(&self, row_idx: usize) -> (Vec<Range<usize>>, Option<usize>) {
        let Some(s) = &self.search else {
            return (Vec::new(), None);
        };
        if s.matches.is_empty() {
            return (Vec::new(), None);
        }
        let cursor_row = s.matches.get(s.cursor).map(|m| m.row_idx);
        let current_is_here = cursor_row == Some(row_idx);
        for m in &s.matches {
            if m.row_idx == row_idx {
                let current = if current_is_here {
                    // The cursor is on this row; pick the first range as
                    // the current (we don't sub-index inside a leaf).
                    Some(0)
                } else {
                    None
                };
                return (m.ranges.clone(), current);
            }
        }
        (Vec::new(), None)
    }

    fn build_search(&self, query: &str) -> ListingSearch {
        let sensitive = smart_case_sensitive(query);
        let mut matches: Vec<LeafMatch> = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            let ranges = find_matches(&row.leaf, query, sensitive);
            if !ranges.is_empty() {
                matches.push(LeafMatch { row_idx: i, ranges });
            }
            if matches.len() >= MAX_MATCHES {
                break;
            }
        }
        ListingSearch { matches, cursor: 0 }
    }

    /// Bring the current match's row into view. When the match is a
    /// file, update the file selection so Extract / Descend target it;
    /// when it's a directory, only scroll.
    fn scroll_to_current_match(&mut self) {
        let Some(s) = &self.search else { return };
        let Some(m) = s.matches.get(s.cursor) else {
            return;
        };
        let row_idx = m.row_idx;
        let is_file = self.rows[row_idx].inner_path.is_some();
        if is_file {
            self.viewport.select_row(&self.rows, row_idx);
        } else {
            self.viewport.scroll_to_row(&self.rows, row_idx);
        }
    }

    fn step_match(&mut self, delta: isize) {
        let Some(s) = self.search.as_mut() else {
            return;
        };
        let n = s.matches.len();
        if n == 0 {
            return;
        }
        let cur = s.cursor as isize;
        let next = ((cur + delta).rem_euclid(n as isize)) as usize;
        s.cursor = next;
        self.scroll_to_current_match();
    }

    /// Mtime column is padded to the widest stringified mtime in the
    /// slice so the path column abuts cleanly. Each row carries its
    /// `self.rows` index so selection highlighting works through the
    /// sticky breadcrumb (parent indices fed in alongside the visible
    /// content slice).
    fn render_slice_with_indices(
        &self,
        slice: &[(usize, TreeRow)],
        theme: &PeekTheme,
        opts: RenderOptions,
        show_mtime: bool,
    ) -> Vec<String> {
        let mtimes: Vec<String> = if show_mtime {
            slice
                .iter()
                .map(|(_, r)| format_mtime(r.mtime.as_ref(), opts.utc))
                .collect()
        } else {
            Vec::new()
        };
        let width = mtimes.iter().map(|s| s.len()).max().unwrap_or(0);
        let selected_idx = self.viewport.selected();
        slice
            .iter()
            .enumerate()
            .map(|(i, (row_idx, row))| {
                let mtime_text = if show_mtime {
                    Some((mtimes[i].as_str(), width))
                } else {
                    None
                };
                let selected = Some(*row_idx) == selected_idx;
                let line = self.paint_row(*row_idx, row, theme, mtime_text, selected);
                if selected {
                    paint_selected_marker(&line, theme)
                } else {
                    format!("  {line}")
                }
            })
            .collect()
    }
}

/// Two-cell caret prefix — paired with a 2-space gutter on
/// non-selected rows so columns stay aligned.
fn paint_selected_marker(line: &str, theme: &PeekTheme) -> String {
    let marker = theme.paint("\u{25b8} ", theme.accent);
    format!("{marker}{line}")
}

impl Mode for ListingMode {
    fn id(&self) -> ModeId {
        ModeId::Listing
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, rows: usize) -> Result<Window> {
        self.viewport.set_viewport_rows(&self.rows, rows);
        let show_mtime = ctx.term_cols >= MTIME_HIDE_BELOW_COLS;
        let win = self.viewport.window(&self.rows);
        // Compose sticky breadcrumb rows + content slice into one
        // buffer so render_slice computes mtime column width across
        // the full visible window — keeps columns aligned. Carry the
        // original row index alongside each row so the selection
        // highlight fires for the right row regardless of sticky
        // displacement.
        let mut combined: Vec<(usize, TreeRow)> =
            Vec::with_capacity(win.sticky.len() + win.content.len());
        for idx in &win.sticky {
            combined.push((*idx, self.rows[*idx].clone()));
        }
        for idx in win.content.clone() {
            combined.push((idx, self.rows[idx].clone()));
        }
        let lines =
            self.render_slice_with_indices(&combined, ctx.peek_theme, ctx.render_opts, show_mtime);
        Ok(Window {
            lines,
            total: self.rows.len(),
        })
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        // Non-interactive: no selection highlight, no marker prefix.
        let show_mtime = ctx.term_cols >= MTIME_HIDE_BELOW_COLS;
        let mtimes: Vec<String> = if show_mtime {
            self.rows
                .iter()
                .map(|r| format_mtime(r.mtime.as_ref(), ctx.render_opts.utc))
                .collect()
        } else {
            Vec::new()
        };
        let width = mtimes.iter().map(|s| s.len()).max().unwrap_or(0);
        for (i, row) in self.rows.iter().enumerate() {
            let mtime_text = if show_mtime {
                Some((mtimes[i].as_str(), width))
            } else {
                None
            };
            out.write_line(&self.paint_row(i, row, ctx.peek_theme, mtime_text, false))?;
        }
        Ok(())
    }

    /// Flat-paths variant for `peek --list`. Files only, no tree
    /// connectors, no directories — each line carries the full
    /// `inner_path` so it can be copy-pasted into `--extract` without
    /// editing.
    fn render_flat_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let theme = ctx.peek_theme;
        for row in &self.rows {
            let Some(path) = &row.inner_path else {
                continue;
            };
            let perms = format_perms(row.mode, row.is_dir);
            let size = format_size(row.size, row.is_dir);
            let painted_perms = paint_perms(&perms, theme);
            let painted_size = paint_size(&size, row.size, row.is_dir, theme);
            let painted_path = theme.paint(path, theme.foreground);
            out.write_line(&format!("{painted_perms}  {painted_size}  {painted_path}"))?;
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
        match action {
            Action::ScrollUp => self.viewport.move_selection(&self.rows, false),
            Action::ScrollDown => self.viewport.move_selection(&self.rows, true),
            Action::PageUp => self.viewport.page(&self.rows, false),
            Action::PageDown => self.viewport.page(&self.rows, true),
            Action::Top => self.viewport.jump_first(&self.rows),
            Action::Bottom => self.viewport.jump_last(&self.rows),
            _ => return false,
        }
        true
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn on_resize(&mut self, _term_cols: usize, term_rows: usize) {
        self.viewport.set_viewport_rows(&self.rows, term_rows);
    }

    fn tracks_position(&self) -> bool {
        true
    }

    fn position(&self) -> Position {
        Position::Line(self.viewport.top())
    }

    fn set_position(&mut self, pos: Position, _source: &InputSource) {
        if let Position::Line(l) = pos {
            self.viewport.set_top(&self.rows, l);
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let files = self.file_count();
        let mut segs = Vec::new();
        let s = match self.viewport.selected_file_pos(&self.rows) {
            Some(pos) => format!("{}/{} ({})", pos, files, self.format_name),
            None => format!("{} ({})", files, self.format_name),
        };
        segs.push((s, theme.muted));
        // Sticky on is the default — only call out the off state.
        if !self.viewport.sticky_enabled() {
            segs.push(("sticky off".to_string(), theme.muted));
        }
        if let Some(search) = &self.search {
            let label = if search.matches.is_empty() {
                "no match".to_string()
            } else {
                format!("match {}/{}", search.cursor + 1, search.matches.len())
            };
            segs.push((label, theme.label));
        }
        segs
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        const ACTIONS: &[HelpEntry] = &[
            (&[Action::ToggleStickyParents], "Pin parent path"),
            (&[Action::Extract], "Extract selected entry"),
            (&[Action::OpenSearch], "Search leaf names"),
            (
                &[Action::NextMatch, Action::PrevMatch],
                "Next / previous match",
            ),
        ];
        ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::ToggleStickyParents => {
                self.viewport.toggle_sticky(&self.rows);
                Handled::Yes
            }
            Action::NextMatch => {
                self.step_match(1);
                Handled::Yes
            }
            Action::PrevMatch => {
                self.step_match(-1);
                Handled::Yes
            }
            Action::Back if self.search.is_some() => {
                self.search = None;
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn set_search(&mut self, query: Option<&str>) -> Option<usize> {
        let query = match query {
            Some(q) if !q.is_empty() => q,
            _ => {
                self.search = None;
                return None;
            }
        };
        let search = self.build_search(query);
        self.search = Some(search);
        self.scroll_to_current_match();
        // ListingMode owns scroll, so no line index to return.
        None
    }

    fn extract_target(&self) -> Option<ExtractTarget> {
        self.viewport
            .selected_inner_path(&self.rows)
            .map(|p| ExtractTarget::EntryPath(p.to_string()))
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
        let is_dir = entry.is_dir();
        let inner_path = (!is_dir).then(|| entry.name.clone());
        rows.push(TreeRow {
            prefix: String::new(),
            leaf: entry.name.clone(),
            is_dir,
            size: entry.size,
            mode: entry.mode,
            mtime: entry.mtime.clone(),
            parent_row: None,
            inner_path,
        });
        if let EntryKind::Dir { children } = &entry.kind {
            let parent = rows.len() - 1;
            walk(children, Some(parent), "", &entry.name, &mut rows);
        }
    }
    rows
}

#[cfg(test)]
pub(super) fn flatten_for_test(entries: &[Entry]) -> Vec<TreeRow> {
    flatten(entries)
}

fn walk(
    entries: &[Entry],
    parent_row: Option<usize>,
    parent_prefix: &str,
    parent_path: &str,
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
        let is_dir = entry.is_dir();
        let inner_full = format!("{parent_path}/{}", entry.name);
        let inner_path = (!is_dir).then(|| inner_full.clone());
        rows.push(TreeRow {
            prefix: format!("{parent_prefix}{connector}"),
            leaf: entry.name.clone(),
            is_dir,
            size: entry.size,
            mode: entry.mode,
            mtime: entry.mtime.clone(),
            parent_row,
            inner_path,
        });
        if let EntryKind::Dir { children } = &entry.kind {
            let cont = if is_last { "  " } else { "\u{2502} " };
            let next_prefix = format!("{parent_prefix}{cont}");
            let new_parent = rows.len() - 1;
            walk(children, Some(new_parent), &next_prefix, &inner_full, rows);
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
/// with a trailing `/` for directory entries. When `selected`, the
/// leaf gets a `selection`-coloured background — a stronger cue than
/// the arrow alone for which row the next extract action will target.
///
/// `match_ranges` (with optional `current_match` index) overlays the
/// search-match background on the matched portion of the leaf. When
/// the row is also selected, the selection bg takes priority (matches
/// repaint over it once the selection bg has been laid down).
fn paint_tree_path(
    prefix: &str,
    leaf: &str,
    is_dir: bool,
    theme: &PeekTheme,
    selected: bool,
    match_ranges: &[Range<usize>],
    current_match: Option<usize>,
) -> String {
    let leaf_color = if is_dir {
        theme.accent
    } else {
        theme.foreground
    };
    let trailing = if is_dir { "/" } else { "" };

    // Paint the leaf, then optionally overlay search-match backgrounds
    // on it. overlay_matches operates on a styled string and skips its
    // SGR escapes, so the foreground colour stays intact outside hits.
    let mut painted_leaf = theme.paint(leaf, leaf_color);
    if !match_ranges.is_empty() {
        painted_leaf = overlay_matches(&painted_leaf, match_ranges, current_match, theme);
    }
    if !trailing.is_empty() {
        painted_leaf.push_str(&theme.paint(trailing, theme.muted));
    }
    if selected {
        painted_leaf = theme.paint_bg(&painted_leaf, theme.selection);
    }
    format!("{}{painted_leaf}", theme.paint(prefix, theme.muted))
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
    fn inner_path_built_for_files_only() {
        let lm = sample();
        let paths: Vec<Option<String>> = lm.rows.iter().map(|r| r.inner_path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                None,                                    // sub/
                None,                                    // sub/deeper/
                Some("sub/deeper/deep.txt".to_string()), // file
                Some("sub/inner.txt".to_string()),       // file
                Some("README.txt".to_string()),          // file
            ]
        );
    }

    #[test]
    fn initial_selection_is_first_file() {
        let lm = sample();
        // Row 2 is the first file row (deep.txt) in the sample tree.
        assert_eq!(lm.viewport.selected(), Some(2));
        assert_eq!(
            lm.viewport.selected_inner_path(&lm.rows),
            Some("sub/deeper/deep.txt")
        );
    }

    #[test]
    fn scroll_down_advances_selection_to_next_file_skipping_dirs() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        lm.scroll(Action::ScrollDown);
        assert_eq!(lm.viewport.selected(), Some(3));
        assert_eq!(
            lm.viewport.selected_inner_path(&lm.rows),
            Some("sub/inner.txt")
        );
        lm.scroll(Action::ScrollDown);
        assert_eq!(lm.viewport.selected(), Some(4));
        assert_eq!(
            lm.viewport.selected_inner_path(&lm.rows),
            Some("README.txt")
        );
        // Past the last file, selection sticks rather than wrapping.
        lm.scroll(Action::ScrollDown);
        assert_eq!(lm.viewport.selected(), Some(4));
    }

    #[test]
    fn scroll_up_walks_back_through_files() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        lm.scroll(Action::Bottom);
        lm.scroll(Action::ScrollUp);
        assert_eq!(lm.viewport.selected(), Some(3));
        lm.scroll(Action::ScrollUp);
        assert_eq!(lm.viewport.selected(), Some(2));
        // First file: stays put.
        lm.scroll(Action::ScrollUp);
        assert_eq!(lm.viewport.selected(), Some(2));
    }

    #[test]
    fn top_and_bottom_jump_to_first_last_file() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        lm.scroll(Action::Bottom);
        assert_eq!(lm.viewport.selected(), Some(4));
        lm.scroll(Action::Top);
        assert_eq!(lm.viewport.selected(), Some(2));
    }

    #[test]
    fn page_down_snaps_selection_to_visible_file() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 2);
        lm.scroll(Action::PageDown);
        let sel = lm.viewport.selected().expect("expected selection");
        let win = lm.viewport.window(&lm.rows);
        assert!(
            win.content.contains(&sel) || win.sticky.contains(&sel),
            "selection {sel} should sit in window {:?}",
            win
        );
        assert!(
            lm.rows[sel].inner_path.is_some(),
            "selection must be a file"
        );
    }

    #[test]
    fn status_segments_show_selected_over_files_total() {
        let lm = sample();
        let tm = crate::theme::ThemeManager::new(
            crate::theme::PeekThemeName::IdeaDark,
            crate::theme::StyleMode::Plain,
        );
        let segs = lm.status_segments(tm.peek_theme());
        // 3 files in sample tree; deep.txt is selected (1st file).
        assert_eq!(segs[0].0, "1/3 (test)");
    }

    fn plain_theme() -> crate::theme::ThemeManager {
        crate::theme::ThemeManager::new(
            crate::theme::PeekThemeName::IdeaDark,
            crate::theme::StyleMode::Plain,
        )
    }

    #[test]
    fn search_matches_files_and_directories_by_leaf() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        // "inner" matches one file leaf.
        lm.set_search(Some("inner"));
        let s = lm.search.as_ref().unwrap();
        assert_eq!(s.matches.len(), 1);
        assert_eq!(s.matches[0].row_idx, 3, "inner.txt is row 3");
        // Selection moved to the file match.
        assert_eq!(lm.viewport.selected(), Some(3));
    }

    #[test]
    fn search_includes_directory_leaves() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        lm.set_search(Some("deeper"));
        let s = lm.search.as_ref().unwrap();
        assert_eq!(s.matches.len(), 1);
        assert_eq!(s.matches[0].row_idx, 1, "deeper/ is row 1");
        // Match is a directory — file selection must stay on the
        // original file (deep.txt = row 2).
        assert_eq!(lm.viewport.selected(), Some(2));
    }

    #[test]
    fn search_leaf_only_no_full_path_matches() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        // "sub/" appears in the joined path but not in any single leaf.
        lm.set_search(Some("sub/"));
        let s = lm.search.as_ref().unwrap();
        assert_eq!(
            s.matches.len(),
            0,
            "search is leaf-scoped — slashes never match"
        );
    }

    #[test]
    fn search_step_cycles_with_wrap() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        // ".txt" appears on every file leaf (3 files).
        lm.set_search(Some(".txt"));
        let s = lm.search.as_ref().unwrap();
        assert_eq!(s.matches.len(), 3);
        assert_eq!(s.cursor, 0);
        let row0 = s.matches[0].row_idx;
        assert_eq!(lm.viewport.selected(), Some(row0));

        lm.handle(Action::NextMatch);
        let s = lm.search.as_ref().unwrap();
        assert_eq!(s.cursor, 1);
        assert_eq!(lm.viewport.selected(), Some(s.matches[1].row_idx));

        lm.handle(Action::NextMatch);
        lm.handle(Action::NextMatch);
        // Wrapped around.
        let s = lm.search.as_ref().unwrap();
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn search_smart_case() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        // All-lowercase → case-insensitive: matches README.txt.
        lm.set_search(Some("readme"));
        assert_eq!(lm.search.as_ref().unwrap().matches.len(), 1);
        // Mixed-case → case-sensitive: original casing must match.
        lm.set_search(Some("README"));
        assert_eq!(lm.search.as_ref().unwrap().matches.len(), 1);
        lm.set_search(Some("Readme"));
        assert_eq!(lm.search.as_ref().unwrap().matches.len(), 0);
    }

    #[test]
    fn back_clears_search() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        lm.set_search(Some("inner"));
        assert!(lm.search.is_some());
        assert_eq!(lm.handle(Action::Back), Handled::Yes);
        assert!(lm.search.is_none());
        // Back with no search falls through.
        assert_eq!(lm.handle(Action::Back), Handled::No);
    }

    #[test]
    fn search_empty_query_clears() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        lm.set_search(Some("inner"));
        assert!(lm.search.is_some());
        lm.set_search(Some(""));
        assert!(lm.search.is_none());
        lm.set_search(Some("inner"));
        assert!(lm.search.is_some());
        lm.set_search(None);
        assert!(lm.search.is_none());
    }

    #[test]
    fn status_segment_shows_search_position() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.rows, 10);
        let tm = plain_theme();
        let theme = tm.peek_theme();
        lm.set_search(Some(".txt"));
        let segs = lm.status_segments(theme);
        assert!(segs.iter().any(|(s, _)| s == "match 1/3"));
        lm.set_search(Some("zzz"));
        let segs = lm.status_segments(theme);
        assert!(segs.iter().any(|(s, _)| s == "no match"));
        lm.set_search(None);
        let segs = lm.status_segments(theme);
        assert!(!segs.iter().any(|(s, _)| s.starts_with("match ")));
        assert!(!segs.iter().any(|(s, _)| s == "no match"));
    }
}
