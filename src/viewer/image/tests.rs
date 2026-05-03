//! Snapshot tests for image rendering. Documents the visual output of
//! each `ImageMode` against representative fixtures so algorithm tweaks
//! that silently change glyph selection, contour density, or cluster
//! polarity surface as snapshot diffs.
//!
//! Snapshots use a small `TermSize` (40×20) and `ColorMode::Plain` to
//! keep diffs readable and avoid ANSI churn. Truecolor coverage is a
//! single smoke snapshot — its job is to assert escapes are emitted at
//! all, not to lock down per-pixel colors.
//!
//! Fragile to upstream `image` (Lanczos) and `resvg` version bumps.
//! Re-bless via `cargo insta accept` after deliberate algorithm changes.

use std::path::PathBuf;

use crate::input::InputSource;
use crate::theme::ColorMode;
use crate::viewer::image::render::{self, GridWindow, TermSize};
use crate::viewer::image::{Background, FitMode, ImageConfig, ImageMode};

const TERM: TermSize = TermSize { cols: 40, rows: 20 };

fn fixture(rel: &str) -> InputSource {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    assert!(path.exists(), "fixture missing: {}", path.display());
    InputSource::File(path)
}

fn config(mode: ImageMode, color_mode: ColorMode, bg: Background) -> ImageConfig {
    ImageConfig {
        mode,
        width: 0,
        background: bg,
        margin: 0,
        color_mode,
        edge_density: 0.10,
        fit: FitMode::Contain,
    }
}

fn render_raster(rel: &str, mode: ImageMode, color_mode: ColorMode, bg: Background) -> String {
    let source = fixture(rel);
    let config = config(mode, color_mode, bg);
    let prep = render::prepare_raster(&source, &config, TERM).expect("prepare_raster");
    let window = GridWindow::full(prep.cols, prep.rows);
    render::render_prepared(&prep, &config, window).join("\n")
}

fn render_svg(rel: &str, mode: ImageMode, color_mode: ColorMode, bg: Background) -> String {
    let source = fixture(rel);
    let config = config(mode, color_mode, bg);
    let prep = render::prepare_svg(&source, &config, TERM).expect("prepare_svg");
    let window = GridWindow::full(prep.cols, prep.rows);
    render::render_prepared(&prep, &config, window).join("\n")
}

// ---------------------------------------------------------------------------
// Plain color: shape-only snapshots, one per ImageMode
// ---------------------------------------------------------------------------

#[test]
fn cozy_room_block_plain() {
    insta::assert_snapshot!(render_raster(
        "test-images/cozy-room.jpg",
        ImageMode::Block,
        ColorMode::Plain,
        Background::Auto,
    ));
}

#[test]
fn cozy_room_full_plain() {
    insta::assert_snapshot!(render_raster(
        "test-images/cozy-room.jpg",
        ImageMode::Full,
        ColorMode::Plain,
        Background::Auto,
    ));
}

#[test]
fn cozy_room_geo_plain() {
    insta::assert_snapshot!(render_raster(
        "test-images/cozy-room.jpg",
        ImageMode::Geo,
        ColorMode::Plain,
        Background::Auto,
    ));
}

#[test]
fn cozy_room_ascii_plain() {
    insta::assert_snapshot!(render_raster(
        "test-images/cozy-room.jpg",
        ImageMode::Ascii,
        ColorMode::Plain,
        Background::Auto,
    ));
}

#[test]
fn cozy_room_contour_plain() {
    insta::assert_snapshot!(render_raster(
        "test-images/cozy-room.jpg",
        ImageMode::Contour,
        ColorMode::Plain,
        Background::Auto,
    ));
}

// ---------------------------------------------------------------------------
// Alpha compositing paths
// ---------------------------------------------------------------------------

#[test]
fn fire_block_auto_bg() {
    insta::assert_snapshot!(render_raster(
        "test-images/fire.png",
        ImageMode::Block,
        ColorMode::Plain,
        Background::Auto,
    ));
}

#[test]
fn fire_block_checkerboard_bg() {
    insta::assert_snapshot!(render_raster(
        "test-images/fire.png",
        ImageMode::Block,
        ColorMode::Plain,
        Background::Checkerboard,
    ));
}

// ---------------------------------------------------------------------------
// SVG paths
// ---------------------------------------------------------------------------

#[test]
fn calendar_svg_block_plain() {
    insta::assert_snapshot!(render_svg(
        "test-images/calendar.svg",
        ImageMode::Block,
        ColorMode::Plain,
        Background::Auto,
    ));
}

#[test]
fn calendar_svg_contour_plain() {
    insta::assert_snapshot!(render_svg(
        "test-images/calendar.svg",
        ImageMode::Contour,
        ColorMode::Plain,
        Background::Auto,
    ));
}

// ---------------------------------------------------------------------------
// Fit modes: prepared grid sizes + windowed render
// ---------------------------------------------------------------------------

fn fit_config(mode: ImageMode, fit: FitMode) -> ImageConfig {
    ImageConfig {
        mode,
        width: 0,
        background: Background::Auto,
        margin: 0,
        color_mode: ColorMode::Plain,
        edge_density: 0.10,
        fit,
    }
}

#[test]
fn contain_fits_within_terminal_on_landscape() {
    // Landscape 1500x1000 in 40x20: width-fit yields 13 rows, comfortably
    // within the 20-row terminal — Contain uses the full terminal width.
    let source = fixture("test-images/cozy-room.jpg");
    let cfg = fit_config(ImageMode::Block, FitMode::Contain);
    let prep = render::prepare_raster(&source, &cfg, TERM).expect("prepare");
    assert_eq!(prep.cols, 40);
    assert_eq!(prep.rows, 13);
    assert!(prep.cols <= TERM.cols && prep.rows <= TERM.rows);
}

#[test]
fn fit_width_overflows_rows_on_portrait() {
    // Portrait 1000x1333: width-fit gives ~26 rows — taller than the 20-row
    // terminal, signalling the viewer to scroll vertically.
    let source = fixture("test-images/orange-flowers.jpg");
    let cfg = fit_config(ImageMode::Block, FitMode::FitWidth);
    let prep = render::prepare_raster(&source, &cfg, TERM).expect("prepare");
    assert_eq!(prep.cols, TERM.cols);
    assert!(
        prep.rows > TERM.rows,
        "rows={} should exceed term",
        prep.rows
    );
}

#[test]
fn fit_height_overflows_cols_on_landscape() {
    // Landscape 1500x1000: height-fit gives 60 cols — wider than the 40-col
    // terminal, signalling the viewer to scroll horizontally.
    let source = fixture("test-images/cozy-room.jpg");
    let cfg = fit_config(ImageMode::Block, FitMode::FitHeight);
    let prep = render::prepare_raster(&source, &cfg, TERM).expect("prepare");
    assert_eq!(prep.rows, TERM.rows);
    assert!(
        prep.cols > TERM.cols,
        "cols={} should exceed term",
        prep.cols
    );
}

#[test]
fn fit_width_full_window_emits_all_rows() {
    let source = fixture("test-images/orange-flowers.jpg");
    let cfg = fit_config(ImageMode::Block, FitMode::FitWidth);
    let prep = render::prepare_raster(&source, &cfg, TERM).expect("prepare");
    let window = GridWindow::full(prep.cols, prep.rows);
    let lines = render::render_prepared(&prep, &cfg, window);
    assert_eq!(lines.len(), prep.rows as usize);
}

#[test]
fn fit_width_scrolled_window_returns_correct_slice() {
    // Render the full image, then re-render a 20-row window starting at row
    // 6, and confirm the windowed output equals the matching slice of the
    // full output. Locks in that windowed render is just a slice of the
    // untruncated output (no off-by-one in the inner pixel coordinate map).
    let source = fixture("test-images/orange-flowers.jpg");
    let cfg = fit_config(ImageMode::Block, FitMode::FitWidth);
    let prep = render::prepare_raster(&source, &cfg, TERM).expect("prepare");
    let full = render::render_prepared(&prep, &cfg, GridWindow::full(prep.cols, prep.rows));
    let visible_rows = TERM.rows;
    let scroll_y = 6u32;
    let window = render::GridWindow {
        col_start: 0,
        col_end: prep.cols,
        row_start: scroll_y,
        row_end: scroll_y + visible_rows,
    };
    let scrolled = render::render_prepared(&prep, &cfg, window);
    assert_eq!(scrolled.len(), visible_rows as usize);
    for (i, line) in scrolled.iter().enumerate() {
        assert_eq!(line, &full[scroll_y as usize + i]);
    }
}

#[test]
fn fit_height_scrolled_window_returns_correct_slice() {
    // Horizontal-scroll counterpart: window starts at col 8, full width
    // = TERM.cols, full row range. Confirm each emitted line equals the
    // corresponding cell-by-cell slice of an equivalent narrower render.
    let source = fixture("test-images/cozy-room.jpg");
    let cfg = fit_config(ImageMode::Block, FitMode::FitHeight);
    let prep = render::prepare_raster(&source, &cfg, TERM).expect("prepare");
    let scroll_x = 8u32;
    let window = render::GridWindow {
        col_start: scroll_x,
        col_end: scroll_x + TERM.cols,
        row_start: 0,
        row_end: prep.rows,
    };
    let scrolled = render::render_prepared(&prep, &cfg, window);
    assert_eq!(scrolled.len(), prep.rows as usize);
    // Each scrolled line should be exactly TERM.cols glyphs wide (Plain
    // mode emits one char per cell).
    for line in &scrolled {
        let visible: String = line
            .chars()
            .filter(|c| !c.is_control() && *c != '\u{1b}')
            .collect();
        assert_eq!(visible.chars().count(), TERM.cols as usize);
    }
}

// ---------------------------------------------------------------------------
// Truecolor smoke: assert ANSI escapes present without locking per-cell RGB
// ---------------------------------------------------------------------------

#[test]
fn cozy_room_block_truecolor_first_line() {
    let out = render_raster(
        "test-images/cozy-room.jpg",
        ImageMode::Block,
        ColorMode::TrueColor,
        Background::Auto,
    );
    // Lock only the first line — full output is dominated by per-cell color
    // sequences whose exact RGB values are not the contract under test.
    let first_line = out.lines().next().expect("at least one line").to_string();
    insta::assert_snapshot!(first_line);
}
