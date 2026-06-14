//! Interactive-viewer UI primitives. Declarations and re-exports only;
//! the code lives in the submodules:
//!
//! - [`keys`]   — `Action` + key bindings + dispatch
//! - [`screen`] — `ScreenBuffer` row-diff frame buffer
//! - [`prompt`] — modal input-line widget
//! - [`help`]   — help-overlay rendering
//! - [`styled`] — the SGR-aware string family (tabs, width, wrap, slice, truncate)
//! - [`status`] — status-line composition
//! - [`term`]   — alternate-screen guard, terminal size + test override

pub mod help;
pub mod keys;
pub mod prompt;
pub mod screen;
pub mod status;
pub mod styled;
pub mod term;

pub use keys::{Action, GLOBAL_ACTIONS, HelpEntry, Outcome};
pub use status::render_themed_status_line;
pub use styled::{
    count_wrap_segments, expand_tabs, slice_styled_h, strip_ansi_width, take_cols, truncate_ansi,
    wrap_styled, wrap_styled_words,
};
pub use term::{content_rows, terminal_cols, terminal_rows, with_alternate_screen};

#[cfg(any(test, feature = "testing"))]
pub use term::test_term_override;

/// Re-exported from `peek-theme` (its real home) — kept here so viewer
/// code keeps one import path for theme construction.
pub use peek_theme::make_peek_theme;
