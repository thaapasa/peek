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
use crate::viewer::modes::{Mode, ModeId, Position, RenderCtx, Window};
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
}

/// One rendered row in the TOC. Holds enough metadata to render
/// without traversing the source tree again.
struct TreeRow {
    /// Composed tree prefix: concatenation of `│   ` / `    ` ancestor
    /// segments plus this row's `├── ` / `└── ` connector. Empty for
    /// top-level rows.
    prefix: String,
    /// Last path segment shown alone — the tree prefix conveys depth.
    leaf: String,
    is_dir: bool,
    size: u64,
    mode: Option<u32>,
    mtime: Option<EntryMtime>,
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
        }
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
        let end = self.top_index.saturating_add(rows).min(self.rows.len());
        let lines = self.render_slice(
            &self.rows[self.top_index..end],
            ctx.peek_theme,
            ctx.render_opts,
            show_mtime,
        );
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
        let s = if total <= viewport {
            format!("{} ({})", total, self.format_name)
        } else {
            format!("{}/{} ({})", self.top_index + 1, total, self.format_name)
        };
        vec![(s, theme.muted)]
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }
}

fn flatten(entries: &[Entry]) -> Vec<TreeRow> {
    // Top level: render flush-left without tree connectors. Every
    // depth-1 row would otherwise carry the same `├── ` / `└── ` at
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
        });
        if let EntryKind::Dir { children } = &entry.kind {
            walk(children, "", &mut rows);
        }
    }
    rows
}

fn walk(entries: &[Entry], parent_prefix: &str, rows: &mut Vec<TreeRow>) {
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
        });
        if let EntryKind::Dir { children } = &entry.kind {
            let cont = if is_last { "  " } else { "\u{2502} " };
            let next_prefix = format!("{parent_prefix}{cont}");
            walk(children, &next_prefix, rows);
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
