//! Archive table-of-contents view. Tree-style listing with permissions,
//! size, mtime, and path — like `tree` joined with `tar -tv`. Listing-only:
//! no payload extraction.

use anyhow::Result;
use crossterm::terminal;
use syntect::highlighting::Color;

use super::reader::{ArchiveEntry, ArchiveMtime, list_entries};
use crate::info::RenderOptions;
use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;
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

pub(crate) struct ArchiveMode {
    format: ArchiveFormat,
    entries: Vec<ArchiveEntry>,
    /// Pre-flattened tree-walk rows. Populated once at construction;
    /// scrolling slices into this without rebuilding.
    rows: Vec<TreeRow>,
    pending_warnings: Vec<String>,
    top_index: usize,
    cached_rows: usize,
    label: String,
}

/// One rendered row in the TOC. Implicit directories (a parent path
/// referenced by a child entry but not stored explicitly in the archive
/// header chain) carry `entry_idx = None` and render with `?` perms.
struct TreeRow {
    /// Index into `ArchiveMode::entries`, or `None` for synthesized
    /// rows (the archive root, implicit parent dirs).
    entry_idx: Option<usize>,
    /// Composed tree prefix: concatenation of `│   ` / `    ` ancestor
    /// segments plus this row's `├── ` / `└── ` connector. Empty for
    /// the root row.
    prefix: String,
    /// Last path segment shown alone — the tree prefix conveys depth,
    /// so prior segments would only repeat parent rows.
    leaf: String,
    is_dir: bool,
}

impl ArchiveMode {
    pub(crate) fn new(source: &InputSource, format: ArchiveFormat) -> Self {
        let (entries, warnings) = match list_entries(source, format) {
            Ok(e) => (e, Vec::new()),
            Err(e) => (Vec::new(), vec![format!("Failed to list archive: {e:#}")]),
        };
        let rows = if entries.is_empty() {
            Vec::new()
        } else {
            flatten_tree(build_tree(&entries))
        };
        let (_, term_rows) = terminal::size().unwrap_or((80, 24));
        Self {
            format,
            entries,
            rows,
            pending_warnings: warnings,
            top_index: 0,
            cached_rows: (term_rows as usize).saturating_sub(1),
            label: "TOC".to_string(),
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
        let entry = row.entry_idx.and_then(|i| self.entries.get(i));
        let perms = format_perms(entry, row.is_dir);
        let size = format_size(entry, row.is_dir);
        let painted_perms = paint_perms(&perms, theme);
        let painted_size = paint_size(&size, entry, row.is_dir, theme);
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
            .map(|r| {
                let entry = r.entry_idx.and_then(|i| self.entries.get(i));
                format_mtime(entry, opts.utc)
            })
            .collect();
        let width = mtimes.iter().map(|s| s.len()).max().unwrap_or(0);
        slice
            .iter()
            .zip(mtimes.iter())
            .map(|(r, m)| self.paint_row(r, theme, Some((m, width))))
            .collect()
    }
}

impl Mode for ArchiveMode {
    fn id(&self) -> ModeId {
        ModeId::Archive
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
            format!("{} ({})", total, self.format.label())
        } else {
            format!("{}/{} ({})", self.top_index + 1, total, self.format.label())
        };
        vec![(s, theme.muted)]
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }
}

/// Tree node assembled from archive entries. Implicit directories
/// (referenced by child paths but not in the archive header chain) get
/// `entry_idx = None` and surface as `?`-perms rows.
struct TreeNode {
    name: String,
    is_dir: bool,
    entry_idx: Option<usize>,
    children: Vec<TreeNode>,
}

fn build_tree(entries: &[ArchiveEntry]) -> TreeNode {
    let mut root = TreeNode {
        name: ".".to_string(),
        is_dir: true,
        entry_idx: None,
        children: Vec::new(),
    };
    for (idx, entry) in entries.iter().enumerate() {
        let trimmed = entry.path.trim_start_matches("./").trim_end_matches('/');
        if trimmed.is_empty() {
            root.entry_idx = Some(idx);
            continue;
        }
        let parts: Vec<&str> = trimmed.split('/').collect();
        insert(&mut root, &parts, idx, entry.is_dir);
    }
    sort_tree(&mut root);
    root
}

fn insert(node: &mut TreeNode, parts: &[&str], idx: usize, is_dir: bool) {
    let (head, tail) = parts.split_first().expect("non-empty parts");
    let pos = match node.children.iter().position(|c| c.name == *head) {
        Some(p) => p,
        None => {
            node.children.push(TreeNode {
                name: head.to_string(),
                is_dir: !tail.is_empty() || is_dir,
                entry_idx: if tail.is_empty() { Some(idx) } else { None },
                children: Vec::new(),
            });
            node.children.len() - 1
        }
    };
    let child = &mut node.children[pos];
    if tail.is_empty() {
        child.entry_idx = Some(idx);
        child.is_dir = is_dir;
    } else {
        insert(child, tail, idx, is_dir);
    }
}

/// Sort children: directories first, then files, alphabetical within
/// each group. Matches `tree --dirsfirst` for predictable layout.
fn sort_tree(node: &mut TreeNode) {
    node.children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    for c in &mut node.children {
        sort_tree(c);
    }
}

fn flatten_tree(root: TreeNode) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    rows.push(TreeRow {
        entry_idx: root.entry_idx,
        prefix: String::new(),
        leaf: root.name.clone(),
        is_dir: true,
    });
    // Skip the first-level tree art: every non-deep entry would otherwise
    // carry the same `├── ` / `└── ` connector at column 0, which is
    // visual noise without payload. Depth-1 rows render flush-left;
    // grandchildren and below still get connectors relative to their
    // top-level parent.
    for child in &root.children {
        rows.push(TreeRow {
            entry_idx: child.entry_idx,
            prefix: String::new(),
            leaf: child.name.clone(),
            is_dir: child.is_dir,
        });
        walk(&child.children, "", &mut rows);
    }
    rows
}

fn walk(children: &[TreeNode], parent_prefix: &str, rows: &mut Vec<TreeRow>) {
    let count = children.len();
    for (i, child) in children.iter().enumerate() {
        let is_last = i + 1 == count;
        let connector = if is_last {
            "\u{2514}\u{2500}\u{2500} "
        } else {
            "\u{251c}\u{2500}\u{2500} "
        };
        rows.push(TreeRow {
            entry_idx: child.entry_idx,
            prefix: format!("{parent_prefix}{connector}"),
            leaf: child.name.clone(),
            is_dir: child.is_dir,
        });
        let cont = if is_last { "    " } else { "\u{2502}   " };
        let next_prefix = format!("{parent_prefix}{cont}");
        walk(&child.children, &next_prefix, rows);
    }
}

/// Render the 10-char `drwxr-xr-x`-style permission string. When mode
/// is unset (implicit tree parents that don't appear in the archive's
/// own header chain), fall back to typical defaults — `rwxr-xr-x` for
/// dirs, `rw-r--r--` for files — so the column stays informative
/// instead of dissolving into a wall of `?`s.
fn format_perms(entry: Option<&ArchiveEntry>, is_dir: bool) -> String {
    let type_ch = if is_dir { 'd' } else { '-' };
    let mode = entry
        .and_then(|e| e.mode)
        .unwrap_or(if is_dir { 0o755 } else { 0o644 });
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

fn format_size(entry: Option<&ArchiveEntry>, is_dir: bool) -> String {
    let raw = match (is_dir, entry) {
        (true, _) | (_, None) => "-".to_string(),
        (false, Some(e)) => crate::info::thousands_sep(e.size),
    };
    format!("{raw:>w$}", w = SIZE_COL_WIDTH)
}

fn format_mtime(entry: Option<&ArchiveEntry>, utc: bool) -> String {
    use std::time::SystemTime;
    let Some(mtime) = entry.and_then(|e| e.mtime.as_ref()) else {
        return "-".to_string();
    };
    match mtime {
        ArchiveMtime::Utc(t) => match t.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => crate::info::format_archive_mtime_zoned(d.as_secs(), utc),
            Err(_) => "-".to_string(),
        },
        ArchiveMtime::LocalNaive {
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

fn paint_size(text: &str, entry: Option<&ArchiveEntry>, is_dir: bool, theme: &PeekTheme) -> String {
    let size = entry.map(|e| e.size).unwrap_or(0);
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
    let trailing = if is_dir && leaf != "." { "/" } else { "" };
    format!(
        "{}{}{}",
        theme.paint(prefix, theme.muted),
        theme.paint(leaf, leaf_color),
        theme.paint(trailing, theme.muted),
    )
}
