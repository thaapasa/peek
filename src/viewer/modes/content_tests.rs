//! Tests for [`ContentMode`](super::ContentMode). Lives in a sibling
//! file but is loaded as a child module of `content` via `#[path]` so
//! it can reach private fields.

use super::super::pretty_view::{PRETTY_MAX_BYTES, PrettyView};
use super::*;
use crate::info::RenderOptions;
use crate::input::detect;
use crate::theme::{PeekTheme, PeekThemeName, StyleMode};
use bytes::Bytes;
use std::path::PathBuf;

/// A pretty branch standing in for the structured pretty-printer: splits
/// on commas so valid input spreads onto multiple lines. Keeps these mode
/// tests independent of `types::structured`.
fn json_pretty() -> PrettyView {
    PrettyView::new(|raw: &str| Ok(raw.replace(',', ",\n")), "JSON")
}

fn fixture(name: &str) -> InputSource {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("test-data");
    path.push(name);
    InputSource::File(path)
}

fn make_ctx<'a>(file_info: &'a crate::info::FileInfo, peek_theme: &'a PeekTheme) -> RenderCtx<'a> {
    RenderCtx {
        file_info,
        theme_name: PeekThemeName::IdeaDark,
        peek_theme,
        render_opts: RenderOptions::default(),
        term_cols: 80,
        term_rows: 24,
    }
}

/// End-to-end: ContentMode's streaming windowed render must match the
/// whole-file `highlight_lines` output for the same line indices.
/// Uses a real Rust fixture so the test goes through the full
/// LineSource → LineStreamHighlighter → ranges_to_escaped path.
#[test]
fn render_window_matches_whole_file_highlight() {
    let source = fixture("theme.rs");
    let detected = detect::detect(&source).unwrap();
    let file_info = crate::info::gather(&source, &detected).unwrap();
    let tm = Rc::new(ThemeManager::new(
        PeekThemeName::IdeaDark,
        StyleMode::TrueColor,
    ));
    let peek_theme = tm.peek_theme().clone();

    let line_source = source.open_line_source().unwrap();
    let total = line_source.total_lines();
    assert!(total > 50, "fixture should have plenty of lines");

    // Reference: whole-file highlight via the same path the pre-A1
    // code used.
    let raw = source.read_text().unwrap();
    let whole = crate::viewer::highlight_lines(
        &raw,
        "rs",
        &tm,
        PeekThemeName::IdeaDark,
        StyleMode::TrueColor,
    )
    .unwrap();

    let mut mode = ContentMode::new(
        source.clone(),
        line_source,
        Rc::clone(&tm),
        PeekThemeName::IdeaDark,
        ContentModeConfig {
            label: "Source",
            syntax_token: Some("rs".to_string()),
            ..Default::default()
        },
    );

    let ctx = make_ctx(&file_info, &peek_theme);

    // Forward scroll: window 0..10 then 10..20 (incremental, no reset).
    // ContentMode owns scroll, so the `scroll` argument to
    // `render_window` is ignored — drive the position via the public
    // field directly. The fixture's longest line (76 cols) fits the
    // 80-col make_ctx width, so soft-wrap doesn't fragment lines and
    // the visual-row output equals the whole-file highlight 1:1.
    let w0 = mode.render_window(&ctx, 0, 10).unwrap();
    assert_eq!(w0.lines.len(), 10);
    assert_eq!(w0.total, total);
    for (i, line) in w0.lines.iter().enumerate() {
        assert_eq!(line, &whole[i], "forward window 0..10 line {i} drift");
    }

    mode.wrap = WrapScroll::for_test(true, 10, 0, 0);
    let w1 = mode.render_window(&ctx, 0, 10).unwrap();
    assert_eq!(w1.lines.len(), 10);
    for (i, line) in w1.lines.iter().enumerate() {
        assert_eq!(line, &whole[10 + i], "forward window 10..20 line {i} drift");
    }

    // Backward jump triggers a highlighter reset; output must still
    // match (this is the regression-prone path — wrong reset and
    // multi-line block-comment highlighting goes sideways).
    mode.wrap = WrapScroll::for_test(true, 0, 0, 0);
    let w_back = mode.render_window(&ctx, 0, 5).unwrap();
    for (i, line) in w_back.lines.iter().enumerate() {
        assert_eq!(line, &whole[i], "backward jump line {i} drift");
    }
}

/// Above the size cap, `ensure_pretty_parsed` should refuse to load,
/// push a warning, and force the rendering back to raw so the user
/// sees the streamed raw view instead.
#[test]
fn pretty_cap_falls_back_to_raw_with_warning() {
    // Pad past PRETTY_MAX_BYTES (16 MB) with valid JSON.
    let mut buf = String::with_capacity(PRETTY_MAX_BYTES as usize + 1024);
    buf.push('[');
    let entry = "0,";
    while (buf.len() as u64) < PRETTY_MAX_BYTES + 64 {
        buf.push_str(entry);
    }
    buf.pop(); // strip trailing comma
    buf.push(']');

    let source = InputSource::stdin(Bytes::from(buf.into_bytes()));
    let line_source = source.open_line_source().unwrap();
    assert!(line_source.total_bytes() > PRETTY_MAX_BYTES);

    let tm = Rc::new(ThemeManager::new(
        PeekThemeName::IdeaDark,
        StyleMode::TrueColor,
    ));

    let mut mode = ContentMode::new(
        source,
        line_source,
        tm,
        PeekThemeName::IdeaDark,
        ContentModeConfig {
            syntax_token: Some("JSON".to_string()),
            pretty: Some(json_pretty()),
            start_pretty: true,
            ..Default::default()
        },
    );

    // Trigger the cap check via ensure_pretty_parsed directly.
    mode.ensure_pretty_parsed();
    assert!(
        !mode.rendering.showing_pretty(),
        "size cap must force back to raw"
    );
    let warnings = mode.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("too large for pretty-print")),
        "expected size-cap warning, got {warnings:?}"
    );
}

/// Build a plain-text ContentMode with no syntax token from inline
/// stdin bytes. Used by the wrap / h-scroll unit tests below — a
/// minimal fixture so the visual-row math is the only moving part.
fn plain_mode_from_bytes(bytes: &[u8]) -> ContentMode {
    let source = InputSource::stdin(Bytes::copy_from_slice(bytes));
    let line_source = source.open_line_source().unwrap();
    let tm = Rc::new(ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain));
    ContentMode::new(
        source,
        line_source,
        tm,
        PeekThemeName::IdeaDark,
        ContentModeConfig {
            label: "Source",
            ..Default::default()
        },
    )
}

/// Wrap-on ScrollDown walks visual rows: advance the sub-row inside
/// the current logical line, then roll over to the next line. With a
/// 1-row viewport and `usable=10`, line 0 (20 cols) has 2 segments
/// and line 1 has 1 segment, so the bottom is `(1, 0)`.
#[test]
fn wrap_on_scrolldown_advances_sub_row_then_logical() {
    let mut mode = plain_mode_from_bytes(b"AAAAAAAAAAAAAAAAAAAA\nBBBB\n");
    mode.cached_cols = 10;
    mode.cached_rows = 1;
    assert!(mode.wrap.soft_wrap(), "default-on");
    assert_eq!((mode.wrap.top_logical(), mode.wrap.top_sub_row()), (0, 0));

    assert!(mode.scroll(Action::ScrollDown));
    assert_eq!((mode.wrap.top_logical(), mode.wrap.top_sub_row()), (0, 1));

    assert!(mode.scroll(Action::ScrollDown));
    assert_eq!((mode.wrap.top_logical(), mode.wrap.top_sub_row()), (1, 0));

    // Past bottom — clamp_top pins us to the bottom position.
    assert!(mode.scroll(Action::ScrollDown));
    assert_eq!((mode.wrap.top_logical(), mode.wrap.top_sub_row()), (1, 0));
}

/// Wrap-on ScrollUp from `(N, 0)` lands on the *last* segment of
/// line N-1, not its segment 0.
#[test]
fn wrap_on_scrollup_lands_on_last_segment_of_previous_line() {
    let mut mode = plain_mode_from_bytes(b"AAAAAAAAAAAAAAAAAAAA\nBBBB\n");
    mode.cached_cols = 10;
    mode.cached_rows = 1;
    mode.wrap = WrapScroll::for_test(true, 1, 0, 0);

    assert!(mode.scroll(Action::ScrollUp));
    // line 0 has 2 segments → last segment index is 1.
    assert_eq!((mode.wrap.top_logical(), mode.wrap.top_sub_row()), (0, 1));

    assert!(mode.scroll(Action::ScrollUp));
    assert_eq!((mode.wrap.top_logical(), mode.wrap.top_sub_row()), (0, 0));

    // Already at top — saturate.
    assert!(mode.scroll(Action::ScrollUp));
    assert_eq!((mode.wrap.top_logical(), mode.wrap.top_sub_row()), (0, 0));
}

/// Wrap-off ScrollRight steps `h_scroll` by `H_SCROLL_STEP` (8 cols)
/// per press; ScrollLeft saturates at zero. Wrap-on Left/Right are
/// inert (covered by exercising ScrollRight while soft_wrap=true).
#[test]
fn wrap_off_scrollright_steps_h_scroll_by_eight() {
    let mut mode = plain_mode_from_bytes(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n");
    mode.cached_cols = 80;
    mode.cached_rows = 5;
    mode.wrap = WrapScroll::for_test(false, 0, 0, 0);

    assert_eq!(mode.wrap.h_scroll(), 0);
    assert!(mode.scroll(Action::ScrollRight));
    assert_eq!(mode.wrap.h_scroll(), 8);
    assert!(mode.scroll(Action::ScrollRight));
    assert_eq!(mode.wrap.h_scroll(), 16);
    assert!(mode.scroll(Action::ScrollLeft));
    assert_eq!(mode.wrap.h_scroll(), 8);

    for _ in 0..5 {
        mode.scroll(Action::ScrollLeft);
    }
    assert_eq!(mode.wrap.h_scroll(), 0);
}

/// Wrap-on Left/Right do not move `h_scroll` — h-scroll is only
/// meaningful when wrap is off.
#[test]
fn wrap_on_left_right_do_not_move_h_scroll() {
    let mut mode = plain_mode_from_bytes(b"AAAAAAAAAAAAAAAAAAAA\n");
    mode.cached_cols = 10;
    mode.cached_rows = 5;
    assert!(mode.wrap.soft_wrap());

    mode.scroll(Action::ScrollRight);
    mode.scroll(Action::ScrollRight);
    assert_eq!(mode.wrap.h_scroll(), 0);
}

/// `ToggleSoftWrap` flips wrap, resets `top_sub_row` and `h_scroll`,
/// preserves `top_logical`. Coherent post-flip viewport.
#[test]
fn toggle_soft_wrap_resets_sub_row_and_h_scroll_preserves_logical() {
    let mut mode = plain_mode_from_bytes(b"AAAAAAAAAAAAAAAAAAAA\nBBBB\n");
    mode.cached_cols = 10;
    mode.cached_rows = 5;
    mode.wrap = WrapScroll::for_test(false, 1, 0, 16);

    let r = mode.handle(Action::ToggleSoftWrap);
    assert_eq!(r, Handled::Yes);
    assert!(mode.wrap.soft_wrap());
    assert_eq!(mode.wrap.top_logical(), 1);
    assert_eq!(mode.wrap.top_sub_row(), 0);
    assert_eq!(mode.wrap.h_scroll(), 0);

    // Flip back: top_logical stays, sub-row + h-scroll already 0.
    let r = mode.handle(Action::ToggleSoftWrap);
    assert_eq!(r, Handled::Yes);
    assert!(!mode.wrap.soft_wrap());
    assert_eq!(mode.wrap.top_logical(), 1);
}

/// `status_segments` emits a `Wrap` segment when wrap is on and
/// nothing extra when off (default-non-default convention).
#[test]
fn status_segments_show_wrap_only_when_on() {
    let mode = plain_mode_from_bytes(b"hi\n");
    let tm = ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain);
    let theme = tm.peek_theme().clone();
    // Default-on.
    let segs = mode.status_segments(&theme);
    assert!(segs.iter().any(|(s, _)| s == "Wrap"));

    let mut mode_off = plain_mode_from_bytes(b"hi\n");
    mode_off.wrap = WrapScroll::for_test(false, 0, 0, 0);
    let segs = mode_off.status_segments(&theme);
    assert!(!segs.iter().any(|(s, _)| s == "Wrap"));
}

/// `set_search` scans the raw branch, records every match in
/// document order, and jumps the viewport to the first match's line.
#[test]
fn set_search_finds_matches_and_jumps() {
    let mut mode = plain_mode_from_bytes(b"alpha\nbeta\ngamma beta\ndelta\n");
    let first = mode.set_search(Some("beta"));
    let search = mode.search.as_ref().expect("search armed");
    assert_eq!(search.match_count(), 2);
    assert_eq!(search.first_line(), Some(1));
    // ContentMode owns its scroll, so the return is always `Owned`.
    assert_eq!(first, crate::viewer::search::SearchTarget::Owned);
    assert_eq!(mode.wrap.top_logical(), 1, "jumped to first match's line");
}

/// `NextMatch` / `PrevMatch` cycle the current-match cursor, wrapping
/// at both ends, and scroll the match's line into view.
#[test]
fn next_prev_match_wrap() {
    let mut mode = plain_mode_from_bytes(b"x\nhit\nx\nhit\n");
    // 1-row viewport so every line is its own scroll position —
    // otherwise the 4-line doc fits whole and clamp pins top at 0.
    mode.cached_cols = 80;
    mode.cached_rows = 1;
    mode.set_search(Some("hit"));
    assert_eq!(mode.search.as_ref().unwrap().match_count(), 2);
    assert_eq!(mode.wrap.top_logical(), 1);

    assert_eq!(mode.handle(Action::NextMatch), Handled::Yes);
    assert_eq!(mode.wrap.top_logical(), 3);

    // Forward past the end wraps to the first match.
    assert_eq!(mode.handle(Action::NextMatch), Handled::Yes);
    assert_eq!(mode.wrap.top_logical(), 1);

    // Backward past the start wraps to the last match.
    assert_eq!(mode.handle(Action::PrevMatch), Handled::Yes);
    assert_eq!(mode.wrap.top_logical(), 3);
}

/// A `None` or empty query clears any active search.
#[test]
fn set_search_none_and_empty_clear() {
    let mut mode = plain_mode_from_bytes(b"foo\nfoo\n");
    mode.set_search(Some("foo"));
    assert!(mode.search.is_some());
    mode.set_search(None);
    assert!(mode.search.is_none());
    mode.set_search(Some("foo"));
    assert!(mode.search.is_some());
    mode.set_search(Some(""));
    assert!(mode.search.is_none());
}

/// Flipping the raw/pretty toggle drops the search — match line
/// indices are in the old branch's domain and mean nothing in the
/// new one.
#[test]
fn toggle_raw_source_clears_search() {
    let source = InputSource::stdin(Bytes::from_static(b"[1,2,1]"));
    let line_source = source.open_line_source().unwrap();
    let tm = Rc::new(ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain));
    let mut mode = ContentMode::new(
        source,
        line_source,
        tm,
        PeekThemeName::IdeaDark,
        ContentModeConfig {
            syntax_token: Some("JSON".to_string()),
            pretty: Some(json_pretty()),
            start_pretty: false, // start raw
            ..Default::default()
        },
    );
    mode.set_search(Some("1"));
    assert!(mode.search.is_some());
    assert_eq!(mode.handle(Action::ToggleRawSource), Handled::Yes);
    assert!(mode.search.is_none(), "raw/pretty toggle clears search");
}

/// `Back` (Esc) clears an active search and is consumed; with no
/// search active it falls through untouched so the global
/// pop-frame / quit behaviour still applies.
#[test]
fn back_clears_search_then_falls_through() {
    let mut mode = plain_mode_from_bytes(b"foo\nfoo\n");
    assert_eq!(
        mode.handle(Action::Back),
        Handled::No,
        "no search: Back untouched"
    );
    mode.set_search(Some("foo"));
    assert!(mode.search.is_some());
    assert_eq!(
        mode.handle(Action::Back),
        Handled::Yes,
        "Esc consumed to clear search"
    );
    assert!(mode.search.is_none());
    assert_eq!(
        mode.handle(Action::Back),
        Handled::No,
        "search cleared: Back falls through again"
    );
}

/// The search position segment appears only while a search is
/// active: `cur/total` when there are matches, `no match` when none.
#[test]
fn status_segments_show_search_position() {
    let mut mode = plain_mode_from_bytes(b"hit\nhit\n");
    let tm = ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain);
    let theme = tm.peek_theme().clone();
    assert!(!mode.status_segments(&theme).iter().any(|(s, _)| s == "1/2"));
    mode.set_search(Some("hit"));
    assert!(mode.status_segments(&theme).iter().any(|(s, _)| s == "1/2"));
    mode.set_search(Some("zzz"));
    assert!(
        mode.status_segments(&theme)
            .iter()
            .any(|(s, _)| s == "no match")
    );
}

/// Regression: a leading TAB must reach the rendered line as spaces, not
/// as a raw `\t`. A raw tab makes the terminal jump the cursor without
/// painting the skipped cells, so stale content (e.g. a prior info
/// screen) shows through the indentation, and the width helpers count it
/// as zero columns, desyncing wrap / scroll geometry. ContentMode renders
/// through those width helpers, which expand tabs to 4-col tab stops.
#[test]
fn tab_indented_line_expands_to_spaces_in_render() {
    let source = InputSource::stdin(Bytes::from_static(b"\tindented\n"));
    let file_info = crate::info::gather(&source, &detect::detect(&source).unwrap()).unwrap();
    let tm = ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain);
    let peek_theme = tm.peek_theme().clone();
    let ctx = make_ctx(&file_info, &peek_theme);

    let mut mode = plain_mode_from_bytes(b"\tindented\n");
    mode.cached_cols = 80;
    mode.cached_rows = 4;
    let window = mode.render_window(&ctx, 0, 4).unwrap();
    let first = &window.lines[0];
    assert!(
        !first.contains('\t'),
        "rendered line must not contain a raw tab: {first:?}"
    );
    assert!(
        first.starts_with("    indented"),
        "leading tab should expand to 4 spaces: {first:?}"
    );
}
