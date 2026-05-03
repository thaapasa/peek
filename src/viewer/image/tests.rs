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
use crate::viewer::image::render::{self, TermSize};
use crate::viewer::image::{Background, ImageConfig, ImageMode};

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
    }
}

fn render_raster(rel: &str, mode: ImageMode, color_mode: ColorMode, bg: Background) -> String {
    let source = fixture(rel);
    let config = config(mode, color_mode, bg);
    let prep = render::prepare_raster(&source, &config, TERM).expect("prepare_raster");
    render::render_prepared(&prep, &config).join("\n")
}

fn render_svg(rel: &str, mode: ImageMode, color_mode: ColorMode, bg: Background) -> String {
    let source = fixture(rel);
    let config = config(mode, color_mode, bg);
    let prep = render::prepare_svg(&source, &config, TERM).expect("prepare_svg");
    render::render_prepared(&prep, &config).join("\n")
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
