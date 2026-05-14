//! One-flat-level directory listing view. Selection moves over every
//! entry (files and dirs alike); Enter targets whichever the user
//! highlighted. Re-targeting onto a subdirectory is handled by
//! `ViewerState::push_extracted` (which collapses the new frame onto
//! the current one when both are directories), so this mode owns no
//! navigation state of its own.

use std::time::SystemTime;

use anyhow::Result;
use syntect::highlighting::Color;

use crate::info::{RenderOptions, format_archive_mtime_zoned, thousands_sep};
use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::{PeekTheme, lerp_color};
use crate::viewer::modes::{ExtractTarget, Mode, ModeId, Position, RenderCtx, Window};
use crate::viewer::ui::{Action, HelpEntry};

use super::read::{DirEntry, DirEntryKind};

/// Synthetic name for the parent-directory row. Selecting it descends
/// to `Path::canonicalize(parent).parent()`, so the user can walk back
/// up the tree without a stack of frames.
pub const PARENT_LINK_NAME: &str = "..";

/// Width of the size column. Matches `ListingMode` for visual parity.
const SIZE_COL_WIDTH: usize = 12;
/// Below this terminal width the mtime column is dropped to leave room
/// for the path. Same threshold as the listing TOC.
const MTIME_HIDE_BELOW_COLS: usize = 80;

pub struct DirectoryMode {
    entries: Vec<DirEntry>,
    /// Pending warnings (e.g. read failure). Drained on first render via
    /// `take_warnings`, then surfaced through Info.
    pending_warnings: Vec<String>,
    /// Selected row (None when entries empty).
    selected: Option<usize>,
    /// Top of viewport (row index).
    top: usize,
    viewport_rows: usize,
}

impl DirectoryMode {
    /// `show_parent_link` prepends a synthetic `..` row when the
    /// canonical path has a parent. Caller computes that once at
    /// compose time so the mode doesn't have to canonicalize on every
    /// rebuild.
    pub fn new(entries: Vec<DirEntry>, warnings: Vec<String>, show_parent_link: bool) -> Self {
        let mut all = Vec::with_capacity(entries.len() + show_parent_link as usize);
        if show_parent_link {
            all.push(parent_link_entry());
        }
        all.extend(entries);
        let selected = (!all.is_empty()).then_some(0);
        Self {
            entries: all,
            pending_warnings: warnings,
            selected,
            top: 0,
            viewport_rows: 0,
        }
    }

    fn max_top(&self) -> usize {
        self.entries.len().saturating_sub(self.viewport_rows.max(1))
    }

    fn reconcile(&mut self) {
        let Some(sel) = self.selected else {
            self.top = 0;
            return;
        };
        let view = self.viewport_rows.max(1);
        if sel < self.top {
            self.top = sel;
        } else if sel >= self.top + view {
            self.top = sel + 1 - view;
        }
        let max = self.max_top();
        if self.top > max {
            self.top = max;
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let Some(sel) = self.selected else { return };
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        let new = (sel as isize + delta).clamp(0, (len - 1) as isize) as usize;
        self.selected = Some(new);
        self.reconcile();
    }

    fn page_selection(&mut self, forward: bool) {
        let step = self.viewport_rows.max(1).saturating_sub(1).max(1) as isize;
        self.move_selection(if forward { step } else { -step });
    }

    fn jump(&mut self, to_end: bool) {
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        self.selected = Some(if to_end { len - 1 } else { 0 });
        self.reconcile();
    }

    fn paint_row(
        &self,
        entry: &DirEntry,
        theme: &PeekTheme,
        opts: RenderOptions,
        mtime_width: Option<usize>,
        selected: bool,
    ) -> String {
        let perms = format_perms(entry);
        let size = format_size(entry);
        let painted_perms = paint_perms(&perms, theme);
        let painted_size = paint_size(&size, entry, theme);
        let painted_name = paint_name(entry, theme, selected);
        let core = match mtime_width {
            Some(width) => {
                let text = format_mtime(entry.mtime, opts.utc);
                let padded = format!("{text:<width$}");
                let painted_mtime = theme.paint(&padded, theme.muted);
                format!("{painted_perms}  {painted_size}  {painted_mtime}  {painted_name}")
            }
            None => format!("{painted_perms}  {painted_size}  {painted_name}"),
        };
        if selected {
            let marker = theme.paint("\u{25b8} ", theme.accent);
            format!("{marker}{core}")
        } else {
            format!("  {core}")
        }
    }
}

impl Mode for DirectoryMode {
    fn id(&self) -> ModeId {
        ModeId::Listing
    }

    fn label(&self) -> &str {
        "Listing"
    }

    fn render_window(&mut self, ctx: &RenderCtx, _scroll: usize, rows: usize) -> Result<Window> {
        self.viewport_rows = rows;
        self.reconcile();
        let show_mtime = ctx.term_cols >= MTIME_HIDE_BELOW_COLS;
        let view = self.viewport_rows.max(1);
        let end = (self.top + view).min(self.entries.len());
        let slice = &self.entries[self.top..end];
        let mtime_width = if show_mtime {
            Some(
                slice
                    .iter()
                    .map(|e| format_mtime(e.mtime, ctx.render_opts.utc).len())
                    .max()
                    .unwrap_or(0),
            )
        } else {
            None
        };
        let lines: Vec<String> = slice
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let row = self.top + i;
                let selected = self.selected == Some(row);
                self.paint_row(e, ctx.peek_theme, ctx.render_opts, mtime_width, selected)
            })
            .collect();
        Ok(Window {
            lines,
            total: self.entries.len(),
        })
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let show_mtime = ctx.term_cols >= MTIME_HIDE_BELOW_COLS;
        let mtime_width = if show_mtime {
            Some(
                self.entries
                    .iter()
                    .map(|e| format_mtime(e.mtime, ctx.render_opts.utc).len())
                    .max()
                    .unwrap_or(0),
            )
        } else {
            None
        };
        for entry in &self.entries {
            let line = self.paint_row(entry, ctx.peek_theme, ctx.render_opts, mtime_width, false);
            out.write_line(&line)?;
        }
        Ok(())
    }

    fn render_flat_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        // `--list` flat view: one path per line for easy piping. Matches
        // ListingMode's flat output style for consistency.
        let theme = ctx.peek_theme;
        for entry in &self.entries {
            let perms = format_perms(entry);
            let size = format_size(entry);
            let painted_perms = paint_perms(&perms, theme);
            let painted_size = paint_size(&size, entry, theme);
            let suffix = if entry.kind == DirEntryKind::Dir {
                "/"
            } else {
                ""
            };
            let painted_name = theme.paint(&format!("{}{}", entry.name, suffix), theme.foreground);
            out.write_line(&format!("{painted_perms}  {painted_size}  {painted_name}"))?;
        }
        Ok(())
    }

    fn total_lines(&self) -> Option<usize> {
        Some(self.entries.len())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        match action {
            Action::ScrollUp => self.move_selection(-1),
            Action::ScrollDown => self.move_selection(1),
            Action::PageUp => self.page_selection(false),
            Action::PageDown => self.page_selection(true),
            Action::Top => self.jump(false),
            Action::Bottom => self.jump(true),
            _ => return false,
        }
        true
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn on_resize(&mut self, _term_cols: usize, term_rows: usize) {
        self.viewport_rows = term_rows;
        self.reconcile();
    }

    fn tracks_position(&self) -> bool {
        true
    }

    fn position(&self) -> Position {
        Position::Line(self.top)
    }

    fn set_position(&mut self, pos: Position, _source: &InputSource) {
        if let Position::Line(l) = pos {
            self.top = l.min(self.max_top());
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let total = self.entries.len();
        let s = match self.selected {
            Some(i) => format!("{}/{} (directory)", i + 1, total),
            None => "empty".to_string(),
        };
        vec![(s, theme.muted)]
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        // Enter (Descend) is global; surface it here so the help screen
        // shows it under this mode too. No mode-private actions.
        &[]
    }

    fn extract_target(&self) -> Option<ExtractTarget> {
        let idx = self.selected?;
        let entry = self.entries.get(idx)?;
        Some(ExtractTarget::EntryPath(entry.name.clone()))
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }
}

fn format_perms(entry: &DirEntry) -> String {
    let type_ch = match (entry.is_symlink, entry.kind) {
        (true, _) => 'l',
        (false, DirEntryKind::Dir) => 'd',
        (false, DirEntryKind::File) => '-',
        (false, DirEntryKind::Other) => '?',
    };
    let mode = entry.mode.unwrap_or(match entry.kind {
        DirEntryKind::Dir => 0o755,
        _ => 0o644,
    });
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

fn format_size(entry: &DirEntry) -> String {
    let raw = if entry.kind == DirEntryKind::Dir {
        "-".to_string()
    } else if entry.stat_error {
        "?".to_string()
    } else {
        thousands_sep(entry.size)
    };
    format!("{raw:>w$}", w = SIZE_COL_WIDTH)
}

fn parent_link_entry() -> DirEntry {
    DirEntry {
        name: PARENT_LINK_NAME.to_string(),
        kind: DirEntryKind::Dir,
        size: 0,
        mtime: None,
        mode: None,
        is_symlink: false,
        stat_error: false,
    }
}

fn format_mtime(mtime: Option<SystemTime>, utc: bool) -> String {
    let Some(t) = mtime else {
        return "-".to_string();
    };
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => format_archive_mtime_zoned(d.as_secs(), utc),
        Err(_) => "-".to_string(),
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
        if (i == 3 || i == 6) && i + 1 < 10 {
            out.push_str(&theme.paint("\u{2500}", lerp_color(theme.muted, theme.background, 0.5)));
        }
    }
    out
}

fn paint_size(text: &str, entry: &DirEntry, theme: &PeekTheme) -> String {
    if entry.kind == DirEntryKind::Dir || entry.size == 0 {
        theme.paint(text, theme.muted)
    } else {
        theme.paint(text, size_color(entry.size, theme))
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

fn paint_name(entry: &DirEntry, theme: &PeekTheme, selected: bool) -> String {
    let leaf_color = if entry.kind == DirEntryKind::Dir {
        theme.accent
    } else {
        theme.foreground
    };
    let trailing = if entry.kind == DirEntryKind::Dir {
        "/"
    } else {
        ""
    };
    if selected {
        let mut buf = String::new();
        theme.paint_into(&mut buf, &entry.name, leaf_color);
        if !trailing.is_empty() {
            theme.paint_into(&mut buf, trailing, theme.muted);
        }
        theme.paint_bg(&buf, theme.selection)
    } else {
        let mut buf = theme.paint(&entry.name, leaf_color);
        if !trailing.is_empty() {
            buf.push_str(&theme.paint(trailing, theme.muted));
        }
        buf
    }
}
