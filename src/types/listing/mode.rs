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
use crate::viewer::modes::{ExtractTarget, Handled, Mode, ModeId, Position, RenderCtx, Window};
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
    /// Index into `rows` of the currently selected file row, or `None`
    /// when the listing has no files at all. Up/Down move this; PgUp/Dn
    /// page-scroll and snap selection to the first file in the new view.
    /// Top/End jump to the first / last file. The selection drives both
    /// the highlighted render and which entry the extract action
    /// targets.
    selected_idx: Option<usize>,
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
    /// Full slash-joined path inside the container for file rows
    /// (`"sub/deeper/deep.txt"`); `None` for directory rows. Drives the
    /// extract action — this is the key handed to `extract::extract`.
    inner_path: Option<String>,
}

impl ListingMode {
    pub fn new(
        format_name: impl Into<String>,
        label: impl Into<String>,
        entries: Vec<Entry>,
        warnings: Vec<String>,
    ) -> Self {
        let rows = flatten(&entries);
        let selected_idx = rows.iter().position(|r| r.inner_path.is_some());
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
            selected_idx,
        }
    }

    /// Inner-container path of the currently selected file, or `None`
    /// when the listing holds no files. Surfaced via `extract_target`
    /// so the viewer-level extract dispatch can pull the path out.
    pub fn selected_inner_path(&self) -> Option<&str> {
        self.selected_idx
            .and_then(|i| self.rows.get(i).and_then(|r| r.inner_path.as_deref()))
    }

    /// Total file count (excludes directories). Used by the status
    /// segment to show "selected n / files m".
    fn file_count(&self) -> usize {
        self.rows.iter().filter(|r| r.inner_path.is_some()).count()
    }

    /// Position of the selected row among files only (1-based). `None`
    /// when nothing is selected. Lets the status line show "3 / 14"
    /// over the file count rather than the row count.
    fn selected_file_pos(&self) -> Option<usize> {
        let sel = self.selected_idx?;
        let mut pos = 0usize;
        for (i, row) in self.rows.iter().enumerate() {
            if row.inner_path.is_some() {
                pos += 1;
                if i == sel {
                    return Some(pos);
                }
            }
        }
        None
    }

    /// Find the nearest file row in the given direction starting from
    /// `from` (exclusive). Returns the original `from` when no file row
    /// exists in that direction — keeps selection sticky at the ends.
    fn next_file_row(&self, from: usize, forward: bool) -> Option<usize> {
        let total = self.rows.len();
        if total == 0 {
            return None;
        }
        if forward {
            (from + 1..total).find(|&i| self.rows[i].inner_path.is_some())
        } else {
            (0..from).rev().find(|&i| self.rows[i].inner_path.is_some())
        }
    }

    fn first_file_row(&self) -> Option<usize> {
        self.rows.iter().position(|r| r.inner_path.is_some())
    }

    fn last_file_row(&self) -> Option<usize> {
        self.rows.iter().rposition(|r| r.inner_path.is_some())
    }

    /// First file row whose render position is at or after `top_index`
    /// — used after a page-scroll so the selection lands on what the
    /// user just scrolled into view.
    fn first_visible_file(&self) -> Option<usize> {
        (self.top_index..self.rows.len()).find(|&i| self.rows[i].inner_path.is_some())
    }

    /// Adjust `top_index` so the selected row is inside the viewport.
    /// Sticky breadcrumbs aren't accounted for here — they reduce the
    /// content slot by at most a third of the viewport, and the result
    /// of overshooting by that small margin is just one extra scroll
    /// step the user can take, not lost selection.
    fn scroll_to_show_selection(&mut self) {
        let Some(sel) = self.selected_idx else {
            return;
        };
        let viewport = self.cached_rows.max(1);
        if sel < self.top_index {
            self.top_index = sel;
        } else if sel >= self.top_index + viewport {
            self.top_index = sel.saturating_sub(viewport - 1);
        }
        let max = self.max_top();
        if self.top_index > max {
            self.top_index = max;
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
        let viewport = self.cached_rows.max(1);
        let total = self.rows.len();
        if total <= viewport {
            return 0;
        }
        // Sticky takes some upper rows of the viewport, so the content
        // slot is `viewport - sticky_len`. The naive bound
        // `total - viewport` would leave the bottom `sticky_len` rows
        // unreachable. Sticky depends on which row is at top, so this
        // is a fixed-point search: try the naive bound, see if the tail
        // still fits, advance by one if not. Bounded by the sticky cap
        // (viewport / 3 + 1) — a handful of iterations at worst.
        let max_iter = (viewport / 3).max(1) + 1;
        let mut top = total - viewport;
        for _ in 0..max_iter {
            let sticky_len = self.sticky_chain_len_at(top);
            let content = viewport.saturating_sub(sticky_len).max(1);
            if top + content >= total {
                return top;
            }
            top = (top + 1).min(total - 1);
        }
        top
    }

    /// Length of the sticky chain that would render with `top` as the
    /// viewport's top row. Same suppression rules as `sticky_chain`
    /// (off when sticky disabled, top is row 0, or the row has no
    /// parent). Used by `max_top` to make scroll math sticky-aware
    /// without allocating the full chain vector.
    fn sticky_chain_len_at(&self, top: usize) -> usize {
        if !self.sticky_enabled || top == 0 || self.rows.is_empty() {
            return 0;
        }
        let cap = (self.cached_rows.max(1) / 3).max(1);
        let mut len = 0usize;
        let mut cur = self.rows[top].parent_row;
        while let Some(p) = cur {
            len += 1;
            cur = self.rows[p].parent_row;
        }
        len.min(cap)
    }

    fn paint_row(
        &self,
        row: &TreeRow,
        theme: &PeekTheme,
        mtime_text: Option<(&str, usize)>,
        selected: bool,
    ) -> String {
        let perms = format_perms(row.mode, row.is_dir);
        let size = format_size(row.size, row.is_dir);
        let painted_perms = paint_perms(&perms, theme);
        let painted_size = paint_size(&size, row.size, row.is_dir, theme);
        let painted_path = paint_tree_path(&row.prefix, &row.leaf, row.is_dir, theme, selected);
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

    /// Render every visible row, padding the mtime column to the
    /// widest formatted string in the slice so the path column always
    /// abuts the mtime gutter without trailing whitespace. Each row
    /// carries its position in `self.rows` so the selection highlight
    /// can fire for the right entry; the sticky breadcrumb rows reuse
    /// this with the original parent indices so a selection sitting
    /// inside a pinned ancestor still lights up.
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
        slice
            .iter()
            .enumerate()
            .map(|(i, (row_idx, row))| {
                let mtime_text = if show_mtime {
                    Some((mtimes[i].as_str(), width))
                } else {
                    None
                };
                let selected = Some(*row_idx) == self.selected_idx;
                let line = self.paint_row(row, theme, mtime_text, selected);
                if selected {
                    paint_selected_marker(&line, theme)
                } else {
                    format!("  {line}")
                }
            })
            .collect()
    }
}

/// Prepend a coloured caret to the rendered line so the selected row
/// is unmistakable even when the rest of the listing carries colour of
/// its own. Two-cell prefix keeps non-selected rows aligned with a
/// matching two-space gutter.
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
        // the full visible window — keeps columns aligned. Carry the
        // original row index alongside each row so the selection
        // highlight fires for the right row regardless of sticky
        // displacement.
        let mut combined: Vec<(usize, TreeRow)> =
            Vec::with_capacity(sticky.len() + (end - self.top_index));
        for idx in &sticky {
            combined.push((*idx, self.rows[*idx].clone()));
        }
        for idx in self.top_index..end {
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
        // Pipe rendering is non-interactive: no selection highlight,
        // no marker prefix. Walk every row with its index so the
        // shared formatter does its mtime-column alignment but skip
        // the selection styling.
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
            out.write_line(&self.paint_row(row, ctx.peek_theme, mtime_text, false))?;
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
        match action {
            Action::ScrollUp => {
                if let Some(cur) = self.selected_idx
                    && let Some(prev) = self.next_file_row(cur, false)
                {
                    self.selected_idx = Some(prev);
                }
                self.scroll_to_show_selection();
            }
            Action::ScrollDown => {
                if let Some(cur) = self.selected_idx
                    && let Some(next) = self.next_file_row(cur, true)
                {
                    self.selected_idx = Some(next);
                }
                self.scroll_to_show_selection();
            }
            // Page keys keep the page-scroll behavior (so quickly
            // moving through long listings still works) but snap the
            // selection to whatever file is now at the top of the
            // viewport — that's what the user just scrolled into view.
            Action::PageUp => {
                self.top_index = self.top_index.saturating_sub(rows.saturating_sub(1));
                if let Some(idx) = self.first_visible_file() {
                    self.selected_idx = Some(idx);
                }
            }
            Action::PageDown => {
                self.top_index = self
                    .top_index
                    .saturating_add(rows.saturating_sub(1))
                    .min(max);
                if let Some(idx) = self.first_visible_file() {
                    self.selected_idx = Some(idx);
                }
            }
            Action::Top => {
                if let Some(idx) = self.first_file_row() {
                    self.selected_idx = Some(idx);
                }
                self.top_index = 0;
                self.scroll_to_show_selection();
            }
            Action::Bottom => {
                if let Some(idx) = self.last_file_row() {
                    self.selected_idx = Some(idx);
                }
                self.top_index = max;
                self.scroll_to_show_selection();
            }
            _ => return false,
        }
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
        let files = self.file_count();
        let mut segs = Vec::new();
        let s = match self.selected_file_pos() {
            Some(pos) => format!("{}/{} ({})", pos, files, self.format_name),
            None => format!("{} ({})", files, self.format_name),
        };
        segs.push((s, theme.muted));
        // Sticky on is the default — only call out the off state.
        if !self.sticky_enabled {
            segs.push(("sticky off".to_string(), theme.muted));
        }
        segs
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        const ACTIONS: &[(Action, &str)] = &[
            (Action::ToggleStickyParents, "Pin parent path"),
            (Action::Extract, "Extract selected entry"),
        ];
        ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        if action == Action::ToggleStickyParents {
            self.sticky_enabled = !self.sticky_enabled;
            return Handled::Yes;
        }
        Handled::No
    }

    fn extract_target(&self) -> Option<ExtractTarget> {
        self.selected_inner_path()
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
fn paint_tree_path(
    prefix: &str,
    leaf: &str,
    is_dir: bool,
    theme: &PeekTheme,
    selected: bool,
) -> String {
    let leaf_color = if is_dir {
        theme.accent
    } else {
        theme.foreground
    };
    let trailing = if is_dir { "/" } else { "" };
    let painted_leaf = if selected {
        // Build the leaf+trailing as one bg-painted run so the
        // highlight covers the dir slash too without a gap.
        let mut buf = String::new();
        theme.paint_into(&mut buf, leaf, leaf_color);
        if !trailing.is_empty() {
            theme.paint_into(&mut buf, trailing, theme.muted);
        }
        theme.paint_bg(&buf, theme.selection)
    } else {
        let mut buf = theme.paint(leaf, leaf_color);
        if !trailing.is_empty() {
            buf.push_str(&theme.paint(trailing, theme.muted));
        }
        buf
    };
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

    /// With sticky on and a small viewport, max_top advances past the
    /// naive `total - viewport` so the bottom-most rows aren't hidden
    /// behind the breadcrumb.
    #[test]
    fn max_top_reserves_for_sticky() {
        let mut lm = sample();
        lm.cached_rows = 3;
        // 5 rows, viewport 3. Naive max_top = 2 → with sticky_len = 1
        // only rows 2,3 visible; row 4 would be unreachable. Iterative
        // search lands at top = 3 (sticky_len = 1, content shows rows
        // 3, 4 — tail visible).
        assert_eq!(lm.max_top(), 3);
    }

    #[test]
    fn max_top_naive_when_sticky_disabled() {
        let mut lm = sample();
        lm.cached_rows = 3;
        lm.sticky_enabled = false;
        assert_eq!(lm.max_top(), 2);
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
        assert_eq!(lm.selected_idx, Some(2));
        assert_eq!(lm.selected_inner_path(), Some("sub/deeper/deep.txt"));
    }

    #[test]
    fn scroll_down_advances_selection_to_next_file_skipping_dirs() {
        let mut lm = sample();
        lm.cached_rows = 10;
        lm.scroll(Action::ScrollDown);
        assert_eq!(lm.selected_idx, Some(3));
        assert_eq!(lm.selected_inner_path(), Some("sub/inner.txt"));
        lm.scroll(Action::ScrollDown);
        assert_eq!(lm.selected_idx, Some(4));
        assert_eq!(lm.selected_inner_path(), Some("README.txt"));
        // Past the last file, selection sticks rather than wrapping.
        lm.scroll(Action::ScrollDown);
        assert_eq!(lm.selected_idx, Some(4));
    }

    #[test]
    fn scroll_up_walks_back_through_files() {
        let mut lm = sample();
        lm.cached_rows = 10;
        lm.selected_idx = Some(4); // README.txt
        lm.scroll(Action::ScrollUp);
        assert_eq!(lm.selected_idx, Some(3));
        lm.scroll(Action::ScrollUp);
        assert_eq!(lm.selected_idx, Some(2));
        // First file: stays put.
        lm.scroll(Action::ScrollUp);
        assert_eq!(lm.selected_idx, Some(2));
    }

    #[test]
    fn top_and_bottom_jump_to_first_last_file() {
        let mut lm = sample();
        lm.cached_rows = 10;
        lm.scroll(Action::Bottom);
        assert_eq!(lm.selected_idx, Some(4));
        lm.scroll(Action::Top);
        assert_eq!(lm.selected_idx, Some(2));
    }

    #[test]
    fn page_down_snaps_selection_to_first_visible_file() {
        let mut lm = sample();
        lm.cached_rows = 2;
        // Page down enough to scroll past the dirs at the top.
        lm.scroll(Action::PageDown);
        let sel = lm.selected_idx.expect("expected selection");
        assert!(
            sel >= lm.top_index,
            "selection {sel} should land at or below new top_index {}",
            lm.top_index
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
            crate::theme::ColorMode::Plain,
        );
        let segs = lm.status_segments(tm.peek_theme());
        // 3 files in sample tree; deep.txt is selected (1st file).
        assert_eq!(segs[0].0, "1/3 (test)");
    }
}
