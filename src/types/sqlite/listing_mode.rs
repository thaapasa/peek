//! SQLite schema listing — wraps the generic [`ListingMode`] and
//! overrides descent so `<entity>.csv` rows open a streaming
//! [`SqliteTableMode`] frame directly, bypassing the extract pipeline.
//!
//! Schema rows (`<entity>.sql`) keep using the extract path: the
//! handler in [`super::extract`] dumps the `CREATE …` DDL into an
//! in-memory `.sql` source and the outer re-detect routes it through
//! the SQL syntax view.
//!
//! Every other [`Mode`] method just forwards to the inner
//! [`ListingMode`] — rendering, scrolling, search, status bar all
//! stay identical to a vanilla listing.

use std::time::Duration;

use anyhow::{Result, anyhow};
use syntect::highlighting::Color;

use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::output::PrintOutput;
use crate::theme::PeekTheme;
use crate::viewer::listing::ListingMode;
use crate::viewer::modes::{
    AboutMode, DescendFrame, ExtractTarget, Handled, InfoMode, Mode, ModeId, Position, RenderCtx,
    Window,
};
use crate::viewer::search::SearchTarget;
use crate::viewer::ui::{Action, HelpEntry};

use super::compose::{KIND_TABLES, KIND_VIEWS};
use super::table_mode::build as build_table_mode;

/// Suffix the listing uses for contents rows. Mirrors what
/// [`super::compose`] emits.
pub(crate) const CONTENTS_SUFFIX: &str = ".csv";

pub(crate) struct SqliteListingMode {
    inner: ListingMode,
    /// Source for the database file. Re-opened on each contents-row
    /// descent so the new frame owns its own read-only connection.
    source: InputSource,
    /// Original detection result — reused as the descended frame's
    /// `detected` so InfoMode still renders the SQLite metadata
    /// section over the same DB.
    detected: Detected,
}

impl SqliteListingMode {
    pub(crate) fn new(inner: ListingMode, source: InputSource, detected: Detected) -> Self {
        Self {
            inner,
            source,
            detected,
        }
    }

    /// Inspect the active row; pick a contents-row `<entity>.csv`
    /// inner_path apart into (kind, entity). `None` for any other
    /// selection — schema rows, group directories, empty selection.
    fn selected_contents_row(&self) -> Option<ContentsTarget> {
        let target = self.inner.extract_target()?;
        let ExtractTarget::EntryPath(key) = target else {
            return None;
        };
        parse_contents_key(&key)
    }

    fn build_contents_frame(&self, target: ContentsTarget) -> Result<DescendFrame> {
        let table = build_table_mode(&self.source, &target.entity)
            .map_err(|e| anyhow!("opening {}: {e:#}", target.entity))?;
        let modes: Vec<Box<dyn Mode>> = vec![
            Box::new(table),
            Box::new(InfoMode::new()),
            Box::new(AboutMode::new()),
        ];
        Ok(DescendFrame {
            source: self.source.clone(),
            detected: self.detected.clone(),
            modes,
        })
    }
}

struct ContentsTarget {
    entity: String,
}

fn parse_contents_key(key: &str) -> Option<ContentsTarget> {
    let (kind_dir, rest) = key.split_once('/')?;
    let name = rest.strip_suffix(CONTENTS_SUFFIX)?;
    if name.is_empty() || name.contains('/') {
        return None;
    }
    // Only tables and views are row-bearing. Indexes / triggers never
    // get a `.csv` row written by compose, but a defensive check here
    // keeps a future bug there from opening a SqliteRowSet on something
    // that has no rows.
    match kind_dir {
        KIND_TABLES | KIND_VIEWS => Some(ContentsTarget {
            entity: name.to_string(),
        }),
        _ => None,
    }
}

impl Mode for SqliteListingMode {
    fn id(&self) -> ModeId {
        self.inner.id()
    }

    fn label(&self) -> &str {
        self.inner.label()
    }

    fn is_aux(&self) -> bool {
        self.inner.is_aux()
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        self.inner.render_window(ctx, scroll, rows)
    }

    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        self.inner.render_to_pipe(ctx, out)
    }

    fn render_flat_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        self.inner.render_flat_to_pipe(ctx, out)
    }

    fn total_lines(&self) -> Option<usize> {
        self.inner.total_lines()
    }

    fn owns_scroll(&self) -> bool {
        self.inner.owns_scroll()
    }

    fn scroll(&mut self, action: Action) -> bool {
        self.inner.scroll(action)
    }

    fn rerender_on_resize(&self) -> bool {
        self.inner.rerender_on_resize()
    }

    fn on_resize(&mut self, term_cols: usize, term_rows: usize) {
        self.inner.on_resize(term_cols, term_rows);
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        self.inner.status_segments(theme)
    }

    fn status_hints(&self, has_return_target: bool) -> Vec<&'static str> {
        self.inner.status_hints(has_return_target)
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        self.inner.extra_actions()
    }

    fn handle(&mut self, action: Action) -> Handled {
        self.inner.handle(action)
    }

    fn next_tick(&self) -> Option<Duration> {
        self.inner.next_tick()
    }

    fn tick(&mut self) -> bool {
        self.inner.tick()
    }

    fn tracks_position(&self) -> bool {
        self.inner.tracks_position()
    }

    fn position(&self) -> Position {
        self.inner.position()
    }

    fn set_position(&mut self, pos: Position, source: &InputSource) {
        self.inner.set_position(pos, source);
    }

    fn take_warnings(&mut self) -> Vec<String> {
        self.inner.take_warnings()
    }

    fn extract_target(&self) -> Option<ExtractTarget> {
        // Schema rows: forward so the extract path runs. Contents
        // rows: hide from the extract path so it can't try to extract
        // them — `build_descend_frame` handles Enter, and the `e`
        // extract command flashes "nothing selected" rather than
        // emitting a malformed key.
        let target = self.inner.extract_target()?;
        match &target {
            ExtractTarget::EntryPath(p) if p.ends_with(CONTENTS_SUFFIX) => None,
            _ => Some(target),
        }
    }

    fn build_descend_frame(&mut self) -> Option<Result<DescendFrame>> {
        let target = self.selected_contents_row()?;
        Some(self.build_contents_frame(target))
    }

    fn set_search(&mut self, query: Option<&str>) -> SearchTarget {
        self.inner.set_search(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_contents_key_accepts_tables_and_views() {
        let t = parse_contents_key("tables/books.csv").expect("tables row");
        assert_eq!(t.entity, "books");
        let v = parse_contents_key("views/popular_authors.csv").expect("views row");
        assert_eq!(v.entity, "popular_authors");
    }

    #[test]
    fn parse_contents_key_rejects_non_row_bearing() {
        assert!(parse_contents_key("indexes/idx_x.csv").is_none());
        assert!(parse_contents_key("triggers/trg.csv").is_none());
    }

    #[test]
    fn parse_contents_key_rejects_schema_rows() {
        assert!(parse_contents_key("tables/books.sql").is_none());
    }

    #[test]
    fn parse_contents_key_rejects_bad_shapes() {
        assert!(parse_contents_key("books.csv").is_none(), "missing kind");
        assert!(parse_contents_key("tables/.csv").is_none(), "empty name");
        assert!(parse_contents_key("tables/a/b.csv").is_none(), "nested");
    }
}
