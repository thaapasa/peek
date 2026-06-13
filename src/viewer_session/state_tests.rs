//! Integration-style tests for `ViewerState`: mode cycling, scroll,
//! descend / stack round-trips, prompt confirm, and render-failure
//! degrade — driven through the same `apply` / `handle` entry points
//! the event loop uses.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use std::rc::Rc;

use clap::Parser;

use peek_detect::Detected;
use peek_foundation::info::RenderOptions;
use peek_foundation::viewer::modes::{Mode, ModeId};
use peek_foundation::viewer::ui::keys::{Action, Outcome};
use peek_foundation::viewer::ui::{content_rows, terminal_cols};
use peek_io::InputSource;

use super::state::{ModeBuilder, ViewerState};
use crate::Args;
use crate::compose::Registry;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn build_state(args_argv: &[&str], source: InputSource, detected: Detected) -> ViewerState {
    let args = Args::parse_from(args_argv);
    let registry = Rc::new(Registry::new(&args.compose_opts()).unwrap());
    let modes = registry.compose_modes(&source, &detected).unwrap();
    let registry_for_builder = registry.clone();
    let mode_builder: ModeBuilder = Box::new(move |s, d| registry_for_builder.compose_modes(s, d));
    ViewerState::new(
        source,
        detected,
        args.theme,
        args.color,
        RenderOptions::default(),
        modes,
        mode_builder,
        args.no_tempfile,
        super::Access::Default,
        None,
    )
    .unwrap()
}

/// Build a session over a compressed source whose decompression is
/// deferred — the state the latency guard produces for a big `.gz` / `.xz`
/// in a Default interactive session, but forced on a small fixture so the
/// size gate is bypassed. `detected` must still be `Compressed` (the
/// un-resolved wrapper).
fn build_deferred_state(
    rel: &str,
    source: InputSource,
    detected: Detected,
    fmt: peek_detect::CompressionFormat,
) -> ViewerState {
    let args = Args::parse_from(["peek", rel]);
    let registry = Rc::new(Registry::new(&args.compose_opts()).unwrap());
    let modes = registry.compose_modes(&source, &detected).unwrap();
    let registry_for_builder = registry.clone();
    let mode_builder: ModeBuilder = Box::new(move |s, d| registry_for_builder.compose_modes(s, d));
    ViewerState::new(
        source,
        detected,
        args.theme,
        args.color,
        RenderOptions::default(),
        modes,
        mode_builder,
        args.no_tempfile,
        super::Access::Default,
        Some(super::Deferred::Decompress(fmt)),
    )
    .unwrap()
}

fn fixture_source(rel: &str) -> InputSource {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push(rel);
    InputSource::File(path)
}

fn active_id(state: &ViewerState) -> ModeId {
    let f = state.frame();
    f.modes[f.active].id()
}

#[test]
fn tab_cycles_svg_view_modes() {
    let source = fixture_source("test-images/calendar.svg");
    let detected = peek_detect::detect(&source).unwrap();
    let mut state = build_state(&["peek", "test-images/calendar.svg"], source, detected);

    assert_eq!(active_id(&state), ModeId::ImageRender);

    state.apply(Action::CycleView).unwrap();
    assert_eq!(active_id(&state), ModeId::Content, "tab → XML source");

    state.apply(Action::CycleView).unwrap();
    assert_eq!(active_id(&state), ModeId::Info, "tab → info");

    state.apply(Action::CycleView).unwrap();
    assert_eq!(
        active_id(&state),
        ModeId::ImageRender,
        "tab wraps back to image"
    );
}

/// A corrupt image can't be decoded. Rendering must not abort the
/// viewer: the active mode degrades to the universal Hex view and the
/// decode error is recorded as a frame warning (surfaced by the Info
/// view and the breadcrumb `!` mark).
#[test]
fn corrupt_image_degrades_to_hex_with_warning() {
    // Pin a narrow viewport so the verbose decode warning is wider
    // than the content area — the Info view must wrap it rather than
    // let the terminal soft-wrap a row the ScreenBuffer miscounts.
    let _term = peek_foundation::viewer::ui::test_term_override::pin(40, 24);

    let source = fixture_source("test-images/corrupt.png");
    let detected = peek_detect::detect(&source).unwrap();
    let mut state = build_state(&["peek", "test-images/corrupt.png"], source, detected);

    // Composes as an image — ImageRender is the home view.
    assert_eq!(active_id(&state), ModeId::ImageRender);

    // The decode fails inside render_window; ensure_active_rendered
    // must swallow it (Ok), not propagate.
    state.ensure_active_rendered().unwrap();

    // Degraded to Hex, with the decode cause captured as a warning.
    assert_eq!(active_id(&state), ModeId::Hex, "fell back to hex view");
    assert!(
        state
            .frame()
            .file_info
            .warnings
            .iter()
            .any(|w| w.contains("CRC error")),
        "decode failure recorded as warning, got {:?}",
        state.frame().file_info.warnings
    );

    // Switch to Info and confirm every rendered line fits the content
    // width — no over-wide line for the terminal to soft-wrap.
    state.apply(Action::SwitchInfo).unwrap();
    assert_eq!(active_id(&state), ModeId::Info);
    state.ensure_active_rendered().unwrap();
    let info_idx = state.frame().active;
    let view = state.frame().views[info_idx].as_ref().unwrap();
    let cols = terminal_cols();
    for line in &view.lines {
        assert!(
            peek_foundation::viewer::ui::strip_ansi_width(line) <= cols,
            "Info line exceeds content width {cols}: {line:?}"
        );
    }
}

#[test]
fn scrolldown_on_info_after_tab_advances_scroll() {
    let source = fixture_source("test-images/calendar.svg");
    let detected = peek_detect::detect(&source).unwrap();
    let mut state = build_state(&["peek", "test-images/calendar.svg"], source, detected);

    state.apply(Action::CycleView).unwrap(); // Content
    state.apply(Action::CycleView).unwrap(); // Info
    assert_eq!(active_id(&state), ModeId::Info);

    state.ensure_active_rendered().unwrap();
    let info_idx = state.frame().active;
    let total = state.frame().views[info_idx].as_ref().unwrap().total;
    let rows = content_rows();
    if total > rows {
        let before = state.frame().scroll[info_idx];
        state.apply(Action::ScrollDown).unwrap();
        let after = state.frame().scroll[info_idx];
        assert_eq!(after, before + 1, "ScrollDown should bump scroll by 1");
    }
}

#[test]
fn static_and_animated_svg_share_source_mode() {
    let static_src = fixture_source("test-images/calendar.svg");
    let static_det = peek_detect::detect(&static_src).unwrap();
    let mut static_state = build_state(
        &["peek", "test-images/calendar.svg"],
        static_src,
        static_det,
    );

    let anim_src = fixture_source("test-images/loader-dots.svg");
    let anim_det = peek_detect::detect(&anim_src).unwrap();
    let mut anim_state = build_state(&["peek", "test-images/loader-dots.svg"], anim_src, anim_det);

    assert_eq!(static_state.frame().modes[0].id(), ModeId::ImageRender);
    assert_eq!(anim_state.frame().modes[0].id(), ModeId::Animation);

    static_state.apply(Action::CycleView).unwrap();
    anim_state.apply(Action::CycleView).unwrap();
    assert_eq!(active_id(&static_state), ModeId::Content);
    assert_eq!(active_id(&anim_state), ModeId::Content);
    assert_eq!(static_state.active_label(), "Source");
    assert_eq!(anim_state.active_label(), "Source");

    let static_segs = static_state.active_status_segments();
    let anim_segs = anim_state.active_status_segments();
    assert!(
        static_segs.iter().any(|(s, _)| s == "Pretty"),
        "static SVG source should show Pretty segment, got {static_segs:?}"
    );
    assert!(
        anim_segs.iter().any(|(s, _)| s == "Pretty"),
        "animated SVG source should show Pretty segment, got {anim_segs:?}"
    );
}

#[test]
fn scrolldown_on_svg_source_shifts_window() {
    // Pin viewport so the assertion below isn't a function of the
    // terminal the test happens to run in (the pretty SVG is ~52
    // lines — a tall console makes `total > rows + 5` flaky).
    let _term = peek_foundation::viewer::ui::test_term_override::pin(80, 21);

    let source = fixture_source("test-images/walking-outside.svg");
    let detected = peek_detect::detect(&source).unwrap();
    let mut state = build_state(
        &["peek", "test-images/walking-outside.svg"],
        source,
        detected,
    );

    state.apply(Action::CycleView).unwrap();
    assert_eq!(active_id(&state), ModeId::Content);

    state.ensure_active_rendered().unwrap();
    let idx = state.frame().active;
    let total = state.frame().views[idx].as_ref().unwrap().total;
    let rows = content_rows();
    assert!(
        total > rows + 5,
        "walking-outside.svg pretty XML must exceed viewport (total={total}, rows={rows})"
    );
    let initial_first = state.frame().views[idx].as_ref().unwrap().lines[0].clone();

    for _ in 0..5 {
        assert!(state.try_active_scroll(Action::ScrollDown));
        state.invalidate_active();
    }
    state.ensure_active_rendered().unwrap();
    let scrolled_first = state.frame().views[idx].as_ref().unwrap().lines[0].clone();
    assert_ne!(
        initial_first, scrolled_first,
        "viewport content should shift after scrolling"
    );
}

/// Descending into an archive entry pushes a new frame; Back pops
/// it. Stack-depth counter reflects the push/pop.
#[test]
fn descend_then_back_round_trips_stack() {
    let source = fixture_source("test-data/archive.zip");
    let detected = peek_detect::detect(&source).unwrap();
    let mut state = build_state(&["peek", "test-data/archive.zip"], source, detected);
    assert_eq!(state.stack_depth(), 1);

    // Listing is the active mode for archives. Selection lands on
    // the first file by default.
    state.apply(Action::Descend).unwrap();
    assert_eq!(state.stack_depth(), 2, "descend pushed a frame");
    assert_eq!(state.breadcrumb().len(), 2);

    let back_outcome = state.apply(Action::Back).unwrap();
    assert!(
        matches!(back_outcome, Outcome::Redraw),
        "back at depth 2 should redraw, not quit"
    );
    assert_eq!(state.stack_depth(), 1);

    // Last back at depth 1 quits.
    let final_back = state.apply(Action::Back).unwrap();
    assert!(matches!(final_back, Outcome::Quit));
}

/// A SQLite table-contents frame reuses the db source, so its
/// breadcrumb must show the table name rather than repeating the
/// db file (`library.sqlite > books`, not `library.sqlite >
/// library.sqlite`).
#[test]
fn sqlite_table_frame_breadcrumb_shows_table_name() {
    let source = fixture_source("test-data/library.sqlite");
    let detected = peek_detect::detect(&source).unwrap();
    let mut state = build_state(
        &["peek", "test-data/library.sqlite"],
        source.clone(),
        detected.clone(),
    );
    assert_eq!(state.breadcrumb(), vec!["library.sqlite".to_string()]);

    // Build the contents descend frame the way the listing does:
    // a row viewer over the parent source, labelled with the table.
    let table = peek_types::types::sqlite::table_mode::build(&source, "books").unwrap();
    let modes: Vec<Box<dyn Mode>> = vec![Box::new(table)];
    let frame = peek_foundation::viewer::modes::DescendFrame {
        source: source.clone(),
        detected,
        modes,
        breadcrumb_label: Some("books".to_string()),
    };
    state.push_direct_frame(frame).unwrap();
    assert_eq!(
        state.breadcrumb(),
        vec!["library.sqlite".to_string(), "books".to_string()],
    );
}

/// Directory descent into a subdirectory must collapse the new
/// frame onto the current one — no stack of dirs to back out of.
/// Descending into a regular file *does* push (so Back returns to
/// the listing), and Esc on a depth-1 directory frame quits.
#[test]
fn directory_subdir_descent_replaces_frame() {
    // src/ has subdirectories. Row 0 is the synthetic `..`; skip
    // past it so we exercise descent into a real child dir.
    let source = fixture_source("src");
    let detected = peek_detect::detect(&source).unwrap();
    assert!(matches!(
        detected.file_type,
        peek_detect::FileType::Directory
    ));
    let mut state = build_state(&["peek", "src"], source, detected);
    assert_eq!(state.stack_depth(), 1);
    state.try_active_scroll(Action::ScrollDown);
    state.apply(Action::Descend).unwrap();
    assert_eq!(state.stack_depth(), 1, "dir → dir descent collapses stack");
    assert!(matches!(
        state.frame().detected.file_type,
        peek_detect::FileType::Directory
    ));
}

/// Descending from a directory into a regular file pushes a new
/// frame so Back returns to the listing.
#[test]
fn directory_file_descent_pushes_frame() {
    // The directory listing sorts dirs first then files, so `Bottom`
    // always lands on a file row regardless of how many
    // subdirectories test-data picks up.
    let source = fixture_source("test-data");
    let detected = peek_detect::detect(&source).unwrap();
    let mut state = build_state(&["peek", "test-data"], source, detected);
    assert_eq!(state.stack_depth(), 1);
    state.try_active_scroll(Action::Bottom);
    state.apply(Action::Descend).unwrap();
    assert_eq!(state.stack_depth(), 2, "dir → file descent pushes a frame");
    let back = state.apply(Action::Back).unwrap();
    assert!(matches!(back, Outcome::Redraw));
    assert_eq!(state.stack_depth(), 1);
}

/// Selecting the synthetic `..` row walks one canonical level up
/// and collapses the frame (still a dir → dir descent).
#[test]
fn directory_parent_link_walks_up() {
    let source = fixture_source("src");
    let detected = peek_detect::detect(&source).unwrap();
    let mut state = build_state(&["peek", "src"], source, detected);
    // `..` is row 0 by construction.
    state.apply(Action::Descend).unwrap();
    assert_eq!(state.stack_depth(), 1, ".. descent stays at depth 1");
    let new_path = state
        .frame()
        .source
        .disk_path()
        .expect("dir source has a path")
        .to_path_buf();
    // `peek <MANIFEST>/src` → `..` → `<MANIFEST>` (the project root).
    let expected_parent = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
    assert_eq!(new_path, expected_parent);
}

/// `/` opens the search prompt; typing a query and pressing Enter
/// confirms it, closes the prompt, and re-renders without quitting.
#[test]
fn search_prompt_confirm_runs_search_without_quitting() {
    let source = fixture_source("test-data/theme.rs");
    let detected = peek_detect::detect(&source).unwrap();
    let mut state = build_state(&["peek", "test-data/theme.rs"], source, detected);
    assert_eq!(active_id(&state), ModeId::Content);

    // `/` opens the prompt.
    let outcome = state.apply(Action::OpenSearch).unwrap();
    assert!(matches!(outcome, Outcome::Redraw));
    assert!(state.prompt_active(), "search prompt should be open");

    // Type "fn" then Enter.
    for c in "fn".chars() {
        state.handle_prompt_key(key(KeyCode::Char(c))).unwrap();
    }
    let redraw = state.handle_prompt_key(key(KeyCode::Enter)).unwrap();
    assert!(redraw, "confirm should request a redraw");
    assert!(!state.prompt_active(), "prompt closes on confirm");

    // The post-confirm render must not panic.
    state.ensure_active_rendered().unwrap();
}

/// Binary files: Tab must round-trip Hex ↔ Info. Without the
/// Info-aware `has_data_primary` check, Info counts as the
/// primary view, Hex stays out of the cycle, and the user gets
/// stuck on Info after the first Tab.
#[test]
fn tab_round_trips_hex_and_info_on_binary() {
    // Synthetic in-memory binary blob (non-UTF8 bytes, no
    // recognised extension) — classified as Binary, so only
    // Hex + Info compose into the mode stack.
    let source = InputSource::memory(bytes::Bytes::from(vec![0xFFu8; 1024]), "blob");
    let detected = peek_detect::detect(&source).unwrap();
    let mut state = build_state(&["peek", "blob"], source, detected);
    assert_eq!(active_id(&state), ModeId::Hex, "binary opens on Hex");

    state.apply(Action::CycleView).unwrap();
    assert_eq!(active_id(&state), ModeId::Info, "Tab goes Hex → Info");

    state.apply(Action::CycleView).unwrap();
    assert_eq!(
        active_id(&state),
        ModeId::Hex,
        "Tab returns Info → Hex on binary (Hex is the only data view)"
    );
}

/// A deferred-decompress frame opens on Info with a load hint, and Enter
/// runs the decompression in place: the frame reseeds to the inner
/// content, the deferred marker clears, and the session unlocks so later
/// guarded ops won't re-prompt.
#[test]
fn deferred_decompress_loads_on_enter() {
    let source = fixture_source("test-data/single.gz");
    let detected = peek_detect::detect(&source).unwrap();
    assert!(
        matches!(detected.file_type, peek_detect::FileType::Compressed(_)),
        "fixture must classify as Compressed before resolve"
    );

    let mut state = build_deferred_state(
        "test-data/single.gz",
        source,
        detected,
        peek_detect::CompressionFormat::Gz,
    );

    // Lands on Info (not the raw Hex view) with the load hint visible.
    assert_eq!(
        active_id(&state),
        ModeId::Info,
        "deferred frame opens on Info"
    );
    assert!(state.frame().deferred.is_some(), "frame marked deferred");
    assert!(state.deferred_hint().is_some(), "load hint present");
    assert_eq!(state.access, super::Access::Default, "session still locked");

    // Enter decompresses in place: inner is plain text → a Content view.
    state.apply(Action::Descend).unwrap();
    assert!(
        state.frame().deferred.is_none(),
        "deferred cleared after load"
    );
    assert_eq!(state.access, super::Access::Unlocked, "session unlocked");
    assert_eq!(
        active_id(&state),
        ModeId::Content,
        "reseeded to the inner content's primary view"
    );
    assert!(state.deferred_hint().is_none(), "hint gone once loaded");
    state.ensure_active_rendered().unwrap();
}

/// A deferred compressed-tar TOC opens on a Hex + Info placeholder — the
/// expensive listing walk is held back — and Enter runs the real compose
/// in place, reseeding to the Listing view and unlocking the session.
#[test]
fn deferred_listing_builds_toc_on_enter() {
    let rel = "test-data/archive.tar.gz";
    let source = fixture_source(rel);
    let detected = peek_detect::detect(&source).unwrap();
    assert!(
        matches!(
            detected.file_type,
            peek_detect::FileType::Archive(peek_detect::ArchiveFormat::TarGz)
        ),
        "fixture must classify as a compressed-tar archive"
    );

    // Build the placeholder the deferral path produces (small fixture, so
    // the size gate is forced rather than tripped).
    let args = Args::parse_from(["peek", rel]);
    let registry = Rc::new(Registry::new(&args.compose_opts()).unwrap());
    let deferred = Some(super::Deferred::Listing(peek_detect::ArchiveFormat::TarGz));
    let modes = super::compose_or_defer(deferred, &source, || {
        registry.compose_modes(&source, &detected)
    })
    .unwrap();
    let registry_for_builder = registry.clone();
    let mode_builder: ModeBuilder = Box::new(move |s, d| registry_for_builder.compose_modes(s, d));
    let mut state = ViewerState::new(
        source,
        detected,
        args.theme,
        args.color,
        RenderOptions::default(),
        modes,
        mode_builder,
        args.no_tempfile,
        super::Access::Default,
        deferred,
    )
    .unwrap();

    // Placeholder: lands on Info, no Listing mode built yet.
    assert_eq!(
        active_id(&state),
        ModeId::Info,
        "deferred TOC opens on Info"
    );
    assert!(state.frame().deferred.is_some(), "frame marked deferred");
    assert!(
        state.frame().mode_index(ModeId::Listing).is_none(),
        "listing not composed until confirmed"
    );

    // Enter builds the TOC in place and reseeds to the Listing view.
    state.apply(Action::Descend).unwrap();
    assert!(
        state.frame().deferred.is_none(),
        "deferred cleared after load"
    );
    assert_eq!(state.access, super::Access::Unlocked, "session unlocked");
    assert_eq!(
        active_id(&state),
        ModeId::Listing,
        "reseeded to the archive listing"
    );
    state.ensure_active_rendered().unwrap();
}
