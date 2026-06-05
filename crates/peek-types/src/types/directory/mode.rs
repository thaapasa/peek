//! One-flat-level directory listing view. Selection moves over every
//! entry (files and dirs alike); Enter targets whichever the user
//! highlighted. Re-targeting onto a subdirectory is handled by
//! `ViewerState::push_extracted` (which collapses the new frame onto
//! the current one when both are directories), so this mode owns no
//! navigation state of its own.

use std::ops::Range;
use std::time::SystemTime;

use anyhow::Result;
use syntect::highlighting::Color;

use crate::info::RenderOptions;
use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::PeekTheme;
use crate::viewer::listing::row::{self, MTIME_HIDE_BELOW_COLS, SizeCell};
use crate::viewer::modes::{
    ExtractTarget, Handled, Mode, ModeId, NEXT_PREV_MATCH_HELP, Position, RenderCtx, Window,
};
use crate::viewer::search::{SearchState, SearchTarget, overlay_matches};
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
    /// Active leaf-name search, if any. Scans every entry's name; the
    /// `line` field on each match is the index into `self.entries`.
    /// Every row is selectable here (flat listing), so navigation just
    /// moves the selection onto the match.
    search: Option<SearchState>,
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
            search: None,
        }
    }

    /// Match ranges (in the row's name bytes) and which one is the
    /// active cursor, for `paint_row`. Empty when no search is active
    /// or the row carries no hits.
    fn name_match_ranges(&self, row: usize) -> (Vec<Range<usize>>, Option<usize>) {
        self.search
            .as_ref()
            .and_then(|s| s.line_overlay(row))
            .unwrap_or_default()
    }

    /// Move the selection onto `row` and scroll it into view.
    fn reveal_match(&mut self, row: usize) {
        self.selected = Some(row);
        self.reconcile();
    }

    fn step_match(&mut self, delta: isize) {
        let Some(row) = self.search.as_mut().and_then(|s| s.step(delta)) else {
            return;
        };
        self.reveal_match(row);
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
        row: usize,
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
        let (ranges, current) = self.name_match_ranges(row);
        let painted_name = paint_name(entry, theme, selected, &ranges, current);
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
                self.paint_row(
                    row,
                    e,
                    ctx.peek_theme,
                    ctx.render_opts,
                    mtime_width,
                    selected,
                )
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
        for (row, entry) in self.entries.iter().enumerate() {
            let line = self.paint_row(
                row,
                entry,
                ctx.peek_theme,
                ctx.render_opts,
                mtime_width,
                false,
            );
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
        let mut segs = vec![(s, theme.muted)];
        if let Some(search) = &self.search {
            segs.push(search.status_segment(theme));
        }
        segs
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        // Enter (Descend) is global; surface it here so the help screen
        // shows it under this mode too.
        const ACTIONS: &[HelpEntry] = &[
            (&[Action::OpenSearch], "Search names"),
            NEXT_PREV_MATCH_HELP,
        ];
        ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
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

    fn set_search(&mut self, query: Option<&str>) -> SearchTarget {
        let query = match query {
            Some(q) if !q.is_empty() => q,
            _ => {
                self.search = None;
                return SearchTarget::Owned;
            }
        };
        let search = SearchState::scan(self.entries.iter().map(|e| e.name.as_str()), query);
        let first = search.first_line();
        self.search = Some(search);
        if let Some(row) = first {
            self.reveal_match(row);
        }
        SearchTarget::Owned
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

/// Paint the entry name (accent for dirs, foreground for files) with a
/// trailing `/` on directories. `match_ranges` (with optional
/// `current_match`) overlays the search-match background on the matched
/// bytes of the name; when the row is also selected the selection bg is
/// laid down last so it reads as the active row.
fn paint_name(
    entry: &DirEntry,
    theme: &PeekTheme,
    selected: bool,
    match_ranges: &[Range<usize>],
    current_match: Option<usize>,
) -> String {
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
    // Paint the name, then overlay search-match backgrounds. overlay_matches
    // skips SGR escapes, so the foreground colour survives outside hits.
    let mut buf = theme.paint(&entry.name, leaf_color);
    if !match_ranges.is_empty() {
        buf = overlay_matches(&buf, match_ranges, current_match, theme);
    }
    if !trailing.is_empty() {
        buf.push_str(&theme.paint(trailing, theme.muted));
    }
    if selected {
        buf = theme.paint_bg(&buf, theme.selection);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> DirEntry {
        DirEntry {
            name: name.to_string(),
            kind: DirEntryKind::File,
            size: 0,
            mtime: None,
            mode: None,
            is_symlink: false,
            stat_error: false,
        }
    }

    fn dir(name: &str) -> DirEntry {
        DirEntry {
            kind: DirEntryKind::Dir,
            ..file(name)
        }
    }

    /// Three entries, no parent link:
    ///   src/        (row 0)
    ///   main.rs     (row 1)
    ///   README.md   (row 2)
    fn sample() -> DirectoryMode {
        let mut m = DirectoryMode::new(
            vec![dir("src"), file("main.rs"), file("README.md")],
            Vec::new(),
            false,
        );
        m.viewport_rows = 10;
        m
    }

    fn plain_theme() -> crate::theme::ThemeManager {
        crate::theme::ThemeManager::new(
            crate::theme::PeekThemeName::IdeaDark,
            crate::theme::StyleMode::Plain,
        )
    }

    #[test]
    fn search_moves_selection_to_match() {
        let mut m = sample();
        m.set_search(Some("main"));
        assert_eq!(m.search.as_ref().unwrap().match_count(), 1);
        assert_eq!(m.selected, Some(1));
    }

    #[test]
    fn search_includes_directories() {
        let mut m = sample();
        m.set_search(Some("src"));
        assert_eq!(m.search.as_ref().unwrap().match_count(), 1);
        assert_eq!(m.selected, Some(0));
    }

    #[test]
    fn search_step_cycles_with_wrap() {
        let mut m = sample();
        // "r" hits src, main.rs, README.md.
        m.set_search(Some("r"));
        assert_eq!(m.search.as_ref().unwrap().match_count(), 3);
        let first = m.selected;
        m.handle(Action::NextMatch);
        assert_ne!(m.selected, first);
        m.handle(Action::NextMatch);
        m.handle(Action::NextMatch);
        assert_eq!(m.selected, first, "wraps back to first match");
        m.handle(Action::PrevMatch);
        assert_ne!(m.selected, first);
    }

    #[test]
    fn search_smart_case() {
        let mut m = sample();
        m.set_search(Some("readme"));
        assert_eq!(m.search.as_ref().unwrap().match_count(), 1);
        m.set_search(Some("README"));
        assert_eq!(m.search.as_ref().unwrap().match_count(), 1);
        m.set_search(Some("Readme"));
        assert_eq!(m.search.as_ref().unwrap().match_count(), 0);
    }

    #[test]
    fn back_clears_search() {
        let mut m = sample();
        m.set_search(Some("main"));
        assert!(m.search.is_some());
        assert_eq!(m.handle(Action::Back), Handled::Yes);
        assert!(m.search.is_none());
        assert_eq!(m.handle(Action::Back), Handled::No);
    }

    #[test]
    fn empty_query_clears() {
        let mut m = sample();
        m.set_search(Some("main"));
        assert!(m.search.is_some());
        m.set_search(Some(""));
        assert!(m.search.is_none());
        m.set_search(Some("main"));
        m.set_search(None);
        assert!(m.search.is_none());
    }

    #[test]
    fn status_segment_shows_search_position() {
        let mut m = sample();
        let tm = plain_theme();
        let theme = tm.peek_theme();
        m.set_search(Some("r"));
        let segs = m.status_segments(theme);
        assert!(segs.iter().any(|(s, _)| s == "1/3"));
        m.set_search(Some("zzz"));
        let segs = m.status_segments(theme);
        assert!(segs.iter().any(|(s, _)| s == "no match"));
        m.set_search(None);
        let segs = m.status_segments(theme);
        assert!(!segs.iter().any(|(s, _)| s == "1/3" || s == "no match"));
    }
}
