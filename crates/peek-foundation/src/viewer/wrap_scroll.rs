//! Wrap-aware scroll position for a logical-line text view.
//!
//! A document view scrolls along three coordinate axes that this module
//! owns:
//!
//! * **logical line** — `top_logical`, the source line at the top of
//!   the viewport;
//! * **visual row** — `top_sub_row`, the wrap segment within
//!   `top_logical` (soft-wrap on only);
//! * **horizontal column** — `h_scroll`, the pan offset (soft-wrap off
//!   only).
//!
//! [`WrapScroll`] keeps the position valid across scrolling, paging,
//! resize, and wrap toggles. The geometry — how a logical line splits
//! into visual rows — reads logical lines through [`LineView`], an
//! enum that names the two concrete shapes a `ContentMode` can hold
//! (streaming raw `LineSource` vs materialised pretty-print cache).
//! `ContentMode` is the only caller today; the enum keeps the geometry
//! methods free of the field-level borrow on `ContentMode` that holds
//! both the line source and the wrap state.

use std::borrow::Cow;

use crate::input::LineSource;
use crate::viewer::ui::count_wrap_segments;

/// Horizontal-scroll step (columns) per Left/Right press when wrap is
/// off. `less -S` feel: small enough to land naturally on indented
/// code, big enough that panning a wide log line isn't 20 keypresses.
pub const H_SCROLL_STEP: usize = 8;

/// The logical-line view a [`WrapScroll`] positions over. Two shapes:
/// the streaming raw `LineSource` or a materialised pretty-print
/// cache. The geometry only ever needs the total count and a line's
/// text (to count its wrap segments).
pub enum LineView<'a> {
    Raw(&'a LineSource),
    Pretty(&'a [String]),
}

impl LineView<'_> {
    pub fn total(&self) -> usize {
        match self {
            LineView::Raw(ls) => ls.total_lines(),
            LineView::Pretty(lines) => lines.len(),
        }
    }

    pub fn line(&self, idx: usize) -> Option<Cow<'_, str>> {
        match self {
            LineView::Raw(ls) => ls
                .window(idx..idx + 1)
                .ok()
                .and_then(|mut v| v.drain(..).next())
                .map(Cow::Owned),
            LineView::Pretty(lines) => lines.get(idx).map(|s| Cow::Borrowed(s.as_str())),
        }
    }
}

/// Wrap-aware scroll position. See the module docs for the three axes.
pub struct WrapScroll {
    soft_wrap: bool,
    top_logical: usize,
    top_sub_row: usize,
    h_scroll: usize,
}

impl WrapScroll {
    pub fn new(soft_wrap: bool) -> Self {
        Self {
            soft_wrap,
            top_logical: 0,
            top_sub_row: 0,
            h_scroll: 0,
        }
    }

    pub fn soft_wrap(&self) -> bool {
        self.soft_wrap
    }

    pub fn top_logical(&self) -> usize {
        self.top_logical
    }

    pub fn h_scroll(&self) -> usize {
        self.h_scroll
    }

    /// Leading wrap segments to skip on the top logical line — the
    /// sub-row offset when wrapping, 0 otherwise.
    pub fn first_skip(&self) -> usize {
        if self.soft_wrap { self.top_sub_row } else { 0 }
    }

    /// Jump to the document top. Leaves `h_scroll` untouched — a
    /// vertical `Top` keeps the horizontal pan, matching `less`.
    pub fn jump_to_top(&mut self) {
        self.top_logical = 0;
        self.top_sub_row = 0;
    }

    /// Pin the top to logical line `line`, segment 0. `h_scroll` is the
    /// caller's concern — search reveal handles it separately.
    pub fn jump_to_line(&mut self, line: usize) {
        self.top_logical = line;
        self.top_sub_row = 0;
    }

    /// Flip soft-wrap. `top_logical` stays put so the viewport keeps its
    /// place; `top_sub_row` / `h_scroll` reset since only one of them is
    /// meaningful per wrap state.
    pub fn toggle_wrap(&mut self) {
        self.soft_wrap = !self.soft_wrap;
        self.top_sub_row = 0;
        self.h_scroll = 0;
    }

    pub fn set_h_scroll(&mut self, h: usize) {
        self.h_scroll = h;
    }

    pub fn clear_h_scroll(&mut self) {
        self.h_scroll = 0;
    }

    /// Pan one [`H_SCROLL_STEP`] left / right. Inert while wrapping.
    pub fn pan_left(&mut self) {
        if !self.soft_wrap {
            self.h_scroll = self.h_scroll.saturating_sub(H_SCROLL_STEP);
        }
    }

    pub fn pan_right(&mut self) {
        if !self.soft_wrap {
            self.h_scroll = self.h_scroll.saturating_add(H_SCROLL_STEP);
        }
    }

    /// Step one visual row down. Wrap-on walks segments within the
    /// current line then rolls to the next; wrap-off bumps the logical
    /// line. Overshoot past EOF is cleaned up by [`clamp`](Self::clamp).
    pub fn step_down(&mut self, lines: &LineView, usable: usize) {
        if lines.total() == 0 {
            return;
        }
        if !self.soft_wrap {
            self.top_logical = self.top_logical.saturating_add(1);
            return;
        }
        let segs = self.segment_count(lines, self.top_logical, usable);
        if self.top_sub_row + 1 < segs {
            self.top_sub_row += 1;
        } else {
            self.top_logical = self.top_logical.saturating_add(1);
            self.top_sub_row = 0;
        }
    }

    /// Step one visual row up. Wrap-on lands on the *last* segment of
    /// the previous logical line.
    pub fn step_up(&mut self, lines: &LineView, usable: usize) {
        if lines.total() == 0 {
            return;
        }
        if !self.soft_wrap {
            self.top_logical = self.top_logical.saturating_sub(1);
            return;
        }
        if self.top_sub_row > 0 {
            self.top_sub_row -= 1;
            return;
        }
        if self.top_logical == 0 {
            return;
        }
        self.top_logical -= 1;
        let segs = self.segment_count(lines, self.top_logical, usable);
        self.top_sub_row = segs.saturating_sub(1);
    }

    /// Jump so the document end sits at the viewport bottom.
    pub fn jump_to_bottom(&mut self, lines: &LineView, usable: usize, rows: usize) {
        let (l, s) = self.bottom(lines, usable, rows);
        self.top_logical = l;
        self.top_sub_row = s;
    }

    /// Re-clamp the position so it never sits past the effective
    /// bottom. Call after every mutation — a resize or theme cycle can
    /// change wrap segment counts and strand the viewport.
    pub fn clamp(&mut self, lines: &LineView, usable: usize, rows: usize) {
        let total = lines.total();
        if total == 0 {
            self.top_logical = 0;
            self.top_sub_row = 0;
            return;
        }
        let (max_l, max_s) = self.bottom(lines, usable, rows.max(1));
        if self.top_logical > max_l {
            self.top_logical = max_l;
            self.top_sub_row = max_s;
        } else if self.top_logical == max_l && self.top_sub_row > max_s {
            self.top_sub_row = max_s;
        }
        if !self.soft_wrap {
            self.top_sub_row = 0;
        }
    }

    /// `(top_logical, top_sub_row)` placing EOF exactly at the viewport
    /// bottom — or the document start when it's shorter than the
    /// viewport. Walks segment counts backward from EOF.
    fn bottom(&self, lines: &LineView, usable: usize, rows: usize) -> (usize, usize) {
        let total = lines.total();
        if total == 0 || rows == 0 {
            return (0, 0);
        }
        if !self.soft_wrap {
            return (total.saturating_sub(rows), 0);
        }
        let mut accum = 0usize;
        let mut idx = total;
        while idx > 0 {
            idx -= 1;
            accum += self.segment_count(lines, idx, usable);
            if accum >= rows {
                return (idx, accum - rows);
            }
        }
        (0, 0)
    }

    /// Wrap-segment count of logical line `idx`. 1 when wrap is off or
    /// the line text isn't reachable.
    fn segment_count(&self, lines: &LineView, idx: usize, usable: usize) -> usize {
        if !self.soft_wrap || usable == 0 {
            return 1;
        }
        lines
            .line(idx)
            .map(|l| count_wrap_segments(&l, usable))
            .unwrap_or(1)
    }
}

#[cfg(test)]
impl WrapScroll {
    pub fn top_sub_row(&self) -> usize {
        self.top_sub_row
    }

    /// Construct at an explicit position — test setup only.
    pub fn for_test(
        soft_wrap: bool,
        top_logical: usize,
        top_sub_row: usize,
        h_scroll: usize,
    ) -> Self {
        Self {
            soft_wrap,
            top_logical,
            top_sub_row,
            h_scroll,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Owned-string fixture; `view()` borrows it into a `LineView` for
    /// the test calls.
    fn lines(specs: &[&str]) -> Vec<String> {
        specs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn step_down_walks_segments_then_rolls_to_next_line() {
        // Line 0 is 20 cols wide → 2 segments at usable width 10.
        let buf = lines(&["AAAAAAAAAAAAAAAAAAAA", "BBBB"]);
        let view = LineView::Pretty(&buf);
        let mut w = WrapScroll::new(true);

        w.step_down(&view, 10);
        assert_eq!((w.top_logical(), w.top_sub_row()), (0, 1));
        w.step_down(&view, 10);
        assert_eq!((w.top_logical(), w.top_sub_row()), (1, 0));
    }

    #[test]
    fn step_up_lands_on_last_segment_of_previous_line() {
        let buf = lines(&["AAAAAAAAAAAAAAAAAAAA", "BBBB"]);
        let view = LineView::Pretty(&buf);
        let mut w = WrapScroll::for_test(true, 1, 0, 0);

        w.step_up(&view, 10);
        // Line 0 wraps to 2 segments → last index is 1.
        assert_eq!((w.top_logical(), w.top_sub_row()), (0, 1));
    }

    #[test]
    fn clamp_pins_overshoot_to_bottom() {
        let buf = lines(&["AAAAAAAAAAAAAAAAAAAA", "BBBB"]);
        let view = LineView::Pretty(&buf);
        // Viewport of 1 visual row: bottom is line 1, segment 0.
        let mut w = WrapScroll::for_test(true, 9, 0, 0);
        w.clamp(&view, 10, 1);
        assert_eq!((w.top_logical(), w.top_sub_row()), (1, 0));
    }

    #[test]
    fn pan_is_inert_while_wrapping() {
        let mut w = WrapScroll::new(true);
        w.pan_right();
        assert_eq!(w.h_scroll(), 0);
        w.toggle_wrap();
        w.pan_right();
        assert_eq!(w.h_scroll(), H_SCROLL_STEP);
    }
}
