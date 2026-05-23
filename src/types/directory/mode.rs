//! One-flat-level directory listing view. Selection moves over every
//! entry (files and dirs alike); Enter targets whichever the user
//! highlighted. Re-targeting onto a subdirectory is handled by
//! `ViewerState::push_extracted` (which collapses the new frame onto
//! the current one when both are directories), so this mode owns no
//! navigation state of its own.

use std::time::SystemTime;

use anyhow::Result;
use syntect::highlighting::Color;

use crate::info::RenderOptions;
use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::PeekTheme;
use crate::viewer::listing::row::{self, MTIME_HIDE_BELOW_COLS, SizeCell};
use crate::viewer::modes::{ExtractTarget, Mode, ModeId, Position, RenderCtx, Window};
use crate::viewer::ui::{Action, HelpEntry};

use super::read::{DirEntry, DirEntryKind};

/// Synthetic name for the parent-directory row. Selecting it descends
/// to `Path::canonicalize(parent).parent()`, so the user can walk back
/// up the tree without a stack of frames.
pub const PARENT_LINK_NAME: &str = "..";

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
        let painted_perms = row::paint_perms(&perms, theme);
        let painted_size =
            row::paint_size(&size, entry.size, entry.kind == DirEntryKind::Dir, theme);
        let painted_name = paint_name(entry, theme, selected);
        let painted_mtime = mtime_width
            .map(|width| row::paint_mtime(&format_mtime(entry.mtime, opts.utc), width, theme));
        let core = row::compose_row(
            &painted_perms,
            &painted_size,
            painted_mtime.as_deref(),
            &painted_name,
        );
        row::with_marker(&core, selected, theme)
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
        let mtime_width = mtime_width_for(slice, show_mtime, ctx.render_opts.utc);
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
        let mtime_width = mtime_width_for(&self.entries, show_mtime, ctx.render_opts.utc);
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
            let painted_perms = row::paint_perms(&perms, theme);
            let painted_size =
                row::paint_size(&size, entry.size, entry.kind == DirEntryKind::Dir, theme);
            let suffix = if entry.kind == DirEntryKind::Dir {
                "/"
            } else {
                ""
            };
            let painted_name = theme.paint(&format!("{}{}", entry.name, suffix), theme.foreground);
            out.write_line(&row::compose_row(
                &painted_perms,
                &painted_size,
                None,
                &painted_name,
            ))?;
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
    row::format_perms(type_ch, entry.mode, entry.kind == DirEntryKind::Dir)
}

fn format_size(entry: &DirEntry) -> String {
    let cell = if entry.kind == DirEntryKind::Dir {
        SizeCell::Dir
    } else if entry.stat_error {
        SizeCell::Unknown
    } else {
        SizeCell::Bytes(entry.size)
    };
    row::format_size(cell)
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
        Ok(d) => row::format_mtime_epoch(d.as_secs(), utc),
        Err(_) => "-".to_string(),
    }
}

/// Widest mtime cell across `entries`, or `None` when mtime is hidden.
fn mtime_width_for(entries: &[DirEntry], show_mtime: bool, utc: bool) -> Option<usize> {
    if !show_mtime {
        return None;
    }
    Some(row::mtime_column_width(
        entries.iter().map(|e| format_mtime(e.mtime, utc)),
    ))
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
