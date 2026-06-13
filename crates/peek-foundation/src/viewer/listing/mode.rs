//! Listing engine: the interactive table-of-contents `Mode`. Owns
//! navigation (scroll + selection + paging + sticky breadcrumb + leaf-name
//! search) and the selection marker, painting the one selectable "name"
//! column itself so search-match and selection highlighting stay in one
//! place. Everything file-shaped — row data, the perms/size/mtime columns,
//! the extract key — lives behind a [`ListSource`]; the engine never names
//! a file field.
//!
//! Scroll + selection state lives in [`super::viewport::ListingViewport`],
//! which keeps the invariants (top in range, selection on a selectable row,
//! selection visible inside the *content* slot — not behind the sticky
//! breadcrumb) under one reconcile path so individual methods can't drift.
//! The viewport runs off a cached [`RowMetaCell`] list (parent + selectable
//! per row) so it never re-queries the source mid-scroll.

use std::ops::Range;

use anyhow::Result;
use syntect::highlighting::Color;

use super::entry::Entry;
use super::row;
use super::source::{ListSource, NameCell, RowMetaCell};
use super::tree_source::TreeListSource;
use super::viewport::ListingViewport;
use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::PeekTheme;
use crate::viewer::modes::{
    DescendFrame, ExtractTarget, Handled, Mode, ModeId, NEXT_PREV_MATCH_HELP, Position, RenderCtx,
    Window,
};
use crate::viewer::search::{SearchState, SearchTarget, overlay_matches};
use crate::viewer::ui::{Action, HelpEntry, slice_styled_h, strip_ansi_width};

/// Columns moved per Left/Right keypress — matches `TableMode`'s pan step.
const H_STEP: usize = 8;

pub struct ListingMode {
    source: Box<dyn ListSource>,
    /// View label (Mode::label) — "TOC" / "Schema" / "Embeds" / "Listing".
    label: String,
    /// Cached navigation metadata, one per source row. The viewport reads
    /// it every scroll tick; querying the source each time would be O(rows).
    meta: Vec<RowMetaCell>,
    /// Cached count of selectable rows, for the status segment.
    selectable_count: usize,
    pending_warnings: Vec<String>,
    /// Horizontal pan offset (leftmost visible column), for reading rows
    /// wider than the terminal — long symbol / file names. Clamped to the
    /// widest on-screen row each render.
    h_scroll: usize,
    viewport: ListingViewport,
    /// Active leaf-name search, if any. Scans every row's name (files +
    /// directories); navigation moves the selection when the match is on a
    /// selectable row, and just scrolls otherwise. Each match's `line` is
    /// the source row index.
    search: Option<SearchState>,
    /// Synthetic-descend override; `None` = standard extract path. When set,
    /// [`Mode::build_descend_frame`] hands the selected row's
    /// [`ExtractTarget`] to the closure, letting a container build a frame
    /// over the *current* source instead of extracting to a temp file (e.g.
    /// SQLite opening a streaming table viewer).
    descend_handler: Option<DescendHandler>,
}

/// Closure installed via [`ListingMode::with_descend_handler`]. Returns
/// `Some(frame)` to push a synthetic frame for the selected row, `None` to
/// fall back to the extract pipeline.
type DescendHandler = Box<dyn FnMut(&ExtractTarget) -> Option<Result<DescendFrame>>>;

impl ListingMode {
    /// Build an engine over any [`ListSource`]. Caches navigation metadata
    /// and seeds the viewport at the first selectable row.
    pub fn from_source(
        source: Box<dyn ListSource>,
        label: impl Into<String>,
        warnings: Vec<String>,
    ) -> Self {
        let meta: Vec<RowMetaCell> = (0..source.len())
            .map(|i| RowMetaCell {
                parent: source.parent(i),
                selectable: source.selectable(i),
            })
            .collect();
        let selectable_count = meta.iter().filter(|m| m.selectable).count();
        let viewport = ListingViewport::new(&meta);
        Self {
            source,
            label: label.into(),
            meta,
            selectable_count,
            pending_warnings: warnings,
            h_scroll: 0,
            viewport,
            search: None,
            descend_handler: None,
        }
    }

    /// Convenience for file-tree sources (archives, embeds, zip-backed
    /// documents): build a [`TreeListSource`] from an [`Entry`] tree.
    /// `format_name` is the status label ("ZIP"), `label` the view name.
    pub fn new(
        format_name: impl Into<String>,
        label: impl Into<String>,
        entries: Vec<Entry>,
        warnings: Vec<String>,
    ) -> Self {
        let source = TreeListSource::new(format_name, entries);
        Self::from_source(Box::new(source), label, warnings)
    }

    /// Install a synthetic-descend handler. The closure receives the
    /// selected row's [`ExtractTarget`] on `Action::Descend`; returning
    /// `Some(frame)` pushes it directly (bypassing extract), `None` defers
    /// to the standard extract path.
    pub fn with_descend_handler(
        mut self,
        handler: impl FnMut(&ExtractTarget) -> Option<Result<DescendFrame>> + 'static,
    ) -> Self {
        self.descend_handler = Some(Box::new(handler));
        self
    }

    /// Inner path / key of the selected row — the extract target.
    fn selected_target(&self) -> Option<ExtractTarget> {
        self.viewport
            .selected()
            .and_then(|i| self.source.extract_target(i))
    }

    /// Match ranges (in the row's name bytes) and which one is the active
    /// cursor, for name painting. Empty when no search or no hits.
    fn name_match_ranges(&self, idx: usize) -> (Vec<Range<usize>>, Option<usize>) {
        self.search
            .as_ref()
            .and_then(|s| s.line_overlay(idx))
            .unwrap_or_default()
    }

    /// Bring `idx` into view. When it's selectable, update the selection so
    /// Extract / Descend target it; otherwise only scroll.
    fn reveal_match(&mut self, idx: usize) {
        if self.meta[idx].selectable {
            self.viewport.select_row(&self.meta, idx);
        } else {
            self.viewport.scroll_to_row(&self.meta, idx);
        }
    }

    fn step_match(&mut self, delta: isize) {
        let Some(idx) = self.search.as_mut().and_then(|s| s.step(delta)) else {
            return;
        };
        self.reveal_match(idx);
    }

    /// Compose one row's bare line (no marker): source-painted columns,
    /// then the engine-painted name with search + selection overlays. The
    /// 2-space column gutter matches [`row::compose_row`].
    fn compose_line(&self, idx: usize, ctx: &RenderCtx, selected: bool) -> String {
        let theme = ctx.peek_theme;
        let cells = self.source.row_cells(idx, ctx);
        let (ranges, current) = self.name_match_ranges(idx);
        let name = paint_name(&cells.name, theme, selected, &ranges, current);
        let prefix = theme.paint(&cells.prefix, theme.muted);
        let mut line = String::new();
        for (i, cell) in cells.left.iter().enumerate() {
            if i > 0 {
                line.push_str("  ");
            }
            line.push_str(cell);
        }
        if !cells.left.is_empty() {
            line.push_str("  ");
        }
        line.push_str(&prefix);
        line.push_str(&name);
        line
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
        self.viewport.set_viewport_rows(&self.meta, rows);
        let win = self.viewport.window(&self.meta);
        let selected = self.viewport.selected();
        // Sticky breadcrumb rows above, content slice below — composed in
        // one pass. Selection only ever lands on a selectable (file) row,
        // which is never in the sticky chain, so sticky rows never light up.
        let mut full = Vec::with_capacity(win.sticky.len() + win.content.len());
        for idx in win.sticky.iter().copied().chain(win.content.clone()) {
            let is_sel = Some(idx) == selected;
            let line = self.compose_line(idx, ctx, is_sel);
            full.push(row::with_marker(&line, is_sel, ctx.peek_theme));
        }
        // Pan + crop to the terminal width. Bound the pan to the widest
        // row on screen so Right can't scroll past the content.
        let max_width = full.iter().map(|l| strip_ansi_width(l)).max().unwrap_or(0);
        self.h_scroll = self.h_scroll.min(max_width.saturating_sub(1));
        let lines = full
            .iter()
            .map(|l| slice_styled_h(l, self.h_scroll, ctx.term_cols))
            .collect();
        Ok(Window {
            lines,
            total: self.meta.len(),
        })
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        // Non-interactive: no selection highlight, no marker prefix.
        for idx in 0..self.meta.len() {
            out.write_line(&self.compose_line(idx, ctx, false))?;
        }
        Ok(())
    }

    fn render_flat_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        for idx in 0..self.source.len() {
            if let Some(line) = self.source.flat_line(idx, ctx.peek_theme) {
                out.write_line(&line)?;
            }
        }
        Ok(())
    }

    fn total_lines(&self) -> Option<usize> {
        Some(self.meta.len())
    }

    fn owns_scroll(&self) -> bool {
        true
    }

    fn scroll(&mut self, action: Action) -> bool {
        match action {
            Action::ScrollUp => self.viewport.move_selection(&self.meta, false),
            Action::ScrollDown => self.viewport.move_selection(&self.meta, true),
            Action::PageUp => self.viewport.page(&self.meta, false),
            Action::PageDown => self.viewport.page(&self.meta, true),
            Action::Top => self.viewport.jump_first(&self.meta),
            Action::Bottom => self.viewport.jump_last(&self.meta),
            // Pan: clamped against on-screen content width in render_window.
            Action::ScrollLeft => self.h_scroll = self.h_scroll.saturating_sub(H_STEP),
            Action::ScrollRight => self.h_scroll = self.h_scroll.saturating_add(H_STEP),
            _ => return false,
        }
        true
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn on_resize(&mut self, _term_cols: usize, term_rows: usize) {
        self.viewport.set_viewport_rows(&self.meta, term_rows);
    }

    fn tracks_position(&self) -> bool {
        true
    }

    fn position(&self) -> Position {
        Position::Line(self.viewport.top())
    }

    fn set_position(&mut self, pos: Position, _source: &InputSource) {
        if let Position::Line(l) = pos {
            self.viewport.set_top(&self.meta, l);
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        let total = self.selectable_count;
        let label = self.source.source_label();
        let mut segs = Vec::new();
        let s = match self.viewport.selected_pos(&self.meta) {
            Some(pos) => format!("{pos}/{total} ({label})"),
            None => format!("{total} ({label})"),
        };
        segs.push((s, theme.muted));
        // Sticky on is the default — only call out the off state.
        if !self.viewport.sticky_enabled() {
            segs.push(("sticky off".to_string(), theme.muted));
        }
        // Horizontal pan offset — shown only when panned.
        if self.h_scroll > 0 {
            segs.push((format!("\u{2192}{}", self.h_scroll), theme.muted));
        }
        if let Some(search) = &self.search {
            segs.push(search.status_segment(theme));
        }
        segs
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        const ACTIONS: &[HelpEntry] = &[
            (&[Action::ToggleStickyParents], "Pin parent path"),
            (
                &[Action::ScrollLeft, Action::ScrollRight],
                "Pan left / right",
            ),
            (&[Action::Extract], "Extract selected entry"),
            (&[Action::OpenSearch], "Search names"),
            NEXT_PREV_MATCH_HELP,
        ];
        ACTIONS
    }

    /// The help card, filtered by the source's [`ListingHelp`] so flat
    /// sources don't advertise the sticky toggle, non-extractable
    /// sources don't advertise extract, and a jump-select source can
    /// name its actual select action. Dispatch keeps the full static
    /// card above — an inert key is a harmless no-op.
    fn help_entries(&self) -> Vec<HelpEntry> {
        let help = self.source.help();
        let mut entries: Vec<HelpEntry> = Vec::new();
        if help.sticky {
            entries.push((&[Action::ToggleStickyParents], "Pin parent path"));
        }
        entries.push((
            &[Action::ScrollLeft, Action::ScrollRight],
            "Pan left / right",
        ));
        if help.extract {
            entries.push((&[Action::Extract], "Extract selected entry"));
        }
        if let Some(select) = help.select {
            entries.push(select);
        }
        entries.push((&[Action::OpenSearch], "Search names"));
        entries.push(NEXT_PREV_MATCH_HELP);
        entries
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::ToggleStickyParents => {
                self.viewport.toggle_sticky(&self.meta);
                Handled::Yes
            }
            Action::Next => {
                self.step_match(1);
                Handled::Yes
            }
            Action::Prev => {
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
        let search = SearchState::scan((0..self.source.len()).map(|i| self.source.name(i)), query);
        let first = search.first_line();
        self.search = Some(search);
        if let Some(idx) = first {
            self.reveal_match(idx);
        }
        SearchTarget::Owned
    }

    fn extract_target(&self) -> Option<ExtractTarget> {
        self.selected_target()
    }

    fn selected_extract_size(&self) -> Option<u64> {
        self.viewport
            .selected()
            .and_then(|i| self.source.extract_size(i))
    }

    fn select_jump(&self) -> Option<(ModeId, Position)> {
        self.viewport
            .selected()
            .and_then(|i| self.source.jump_target(i))
    }

    fn build_descend_frame(&mut self) -> Option<Result<DescendFrame>> {
        // Compute the target first so its immutable borrow ends before the
        // handler's `&mut` borrow begins.
        let target = self.selected_target()?;
        let handler = self.descend_handler.as_mut()?;
        handler(&target)
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }
}

/// Paint the name column: accent for dirs (with a trailing `/`), foreground
/// for files. `match_ranges` (with optional `current_match`) overlays the
/// search-match background on the matched bytes; when the row is selected
/// the selection bg is laid down last so it reads as the active row.
fn paint_name(
    name: &NameCell,
    theme: &PeekTheme,
    selected: bool,
    match_ranges: &[Range<usize>],
    current_match: Option<usize>,
) -> String {
    let color = if name.is_dir {
        theme.accent
    } else {
        theme.foreground
    };
    // Paint the leaf, then overlay search-match backgrounds. overlay_matches
    // skips SGR escapes, so the foreground colour survives outside hits.
    let mut painted = theme.paint(&name.text, color);
    if !match_ranges.is_empty() {
        painted = overlay_matches(&painted, match_ranges, current_match, theme);
    }
    if name.is_dir {
        painted.push_str(&theme.paint("/", theme.muted));
    }
    if selected {
        painted = theme.paint_bg(&painted, theme.selection);
    }
    painted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::listing::entry::EntryKind;

    impl ListingMode {
        fn selected_path(&self) -> Option<String> {
            match self
                .viewport
                .selected()
                .and_then(|i| self.source.extract_target(i))
            {
                Some(ExtractTarget::EntryPath(p)) => Some(p),
                _ => None,
            }
        }
    }

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
    fn initial_selection_is_first_file() {
        let lm = sample();
        // Row 2 is the first file row (deep.txt) in the sample tree.
        assert_eq!(lm.viewport.selected(), Some(2));
        assert_eq!(lm.selected_path().as_deref(), Some("sub/deeper/deep.txt"));
    }

    #[test]
    fn scroll_down_advances_selection_to_next_file_skipping_dirs() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.meta, 10);
        lm.scroll(Action::ScrollDown);
        assert_eq!(lm.viewport.selected(), Some(3));
        assert_eq!(lm.selected_path().as_deref(), Some("sub/inner.txt"));
        lm.scroll(Action::ScrollDown);
        assert_eq!(lm.viewport.selected(), Some(4));
        assert_eq!(lm.selected_path().as_deref(), Some("README.txt"));
        // Past the last file, selection sticks rather than wrapping.
        lm.scroll(Action::ScrollDown);
        assert_eq!(lm.viewport.selected(), Some(4));
    }

    #[test]
    fn scroll_up_walks_back_through_files() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.meta, 10);
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
        lm.viewport.set_viewport_rows(&lm.meta, 10);
        lm.scroll(Action::Bottom);
        assert_eq!(lm.viewport.selected(), Some(4));
        lm.scroll(Action::Top);
        assert_eq!(lm.viewport.selected(), Some(2));
    }

    #[test]
    fn search_moves_selection_to_matching_file() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.meta, 10);
        lm.set_search(Some("inner"));
        assert_eq!(lm.viewport.selected(), Some(3));
        assert_eq!(lm.selected_path().as_deref(), Some("sub/inner.txt"));
    }

    #[test]
    fn search_on_directory_scrolls_without_changing_selection() {
        let mut lm = sample();
        lm.viewport.set_viewport_rows(&lm.meta, 10);
        let before = lm.viewport.selected();
        lm.set_search(Some("deeper")); // a directory row
        // Selection (file-only) unchanged; the dir is just scrolled in.
        assert_eq!(lm.viewport.selected(), before);
    }

    #[test]
    fn status_segment_counts_files_only() {
        let lm = sample();
        // 3 files in the tree (deep.txt, inner.txt, README.txt).
        assert_eq!(lm.selectable_count, 3);
    }

    /// Minimal flat jump-select source (the symbol-list shape): nothing
    /// extracts, no row has a parent, selecting jumps.
    struct FlatJumpSource;

    impl ListSource for FlatJumpSource {
        fn len(&self) -> usize {
            1
        }
        fn parent(&self, _idx: usize) -> Option<usize> {
            None
        }
        fn selectable(&self, _idx: usize) -> bool {
            true
        }
        fn name(&self, _idx: usize) -> &str {
            "sym"
        }
        fn row_cells(&self, _idx: usize, _ctx: &RenderCtx) -> super::super::source::RowCells {
            super::super::source::RowCells {
                prefix: String::new(),
                left: Vec::new(),
                name: NameCell {
                    text: "sym".into(),
                    is_dir: false,
                },
            }
        }
        fn extract_target(&self, _idx: usize) -> Option<ExtractTarget> {
            None
        }
        fn flat_line(&self, _idx: usize, _theme: &PeekTheme) -> Option<String> {
            None
        }
        fn source_label(&self) -> &str {
            "test"
        }
        fn help(&self) -> super::super::source::ListingHelp {
            super::super::source::ListingHelp {
                extract: false,
                sticky: false,
                select: Some((&[Action::Descend], "Jump to symbol in hex")),
            }
        }
    }

    fn help_descriptions(lm: &ListingMode) -> Vec<&'static str> {
        lm.help_entries().iter().map(|(_, d)| *d).collect()
    }

    #[test]
    fn tree_help_card_advertises_sticky_and_extract() {
        let lm = sample();
        let descs = help_descriptions(&lm);
        assert!(descs.contains(&"Pin parent path"));
        assert!(descs.contains(&"Extract selected entry"));
        assert!(descs.contains(&"Search names"));
    }

    #[test]
    fn flat_jump_help_card_drops_inert_actions_and_names_the_jump() {
        let lm = ListingMode::from_source(Box::new(FlatJumpSource), "Symbols", Vec::new());
        let descs = help_descriptions(&lm);
        assert!(!descs.contains(&"Pin parent path"));
        assert!(!descs.contains(&"Extract selected entry"));
        assert!(descs.contains(&"Jump to symbol in hex"));
        assert!(descs.contains(&"Search names"));
    }
}
