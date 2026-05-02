//! Composable view modes.
//!
//! A `Mode` is one renderable + interactive view of a file (content,
//! file info, help, hex dump, image render, structured source, etc.).
//! The interactive viewer is configured with a list of modes per file
//! type — Tab cycles between them, `i` jumps to Info, `h` toggles Help.
//!
//! This module currently only hosts `InfoMode` and `HelpMode` while the
//! larger migration is in progress; the rest of the viewers still go
//! through `ViewMode` in `ui/state.rs`.

use std::time::Duration;

use anyhow::Result;
use syntect::highlighting::Color;

use crate::info::{FileInfo, RenderOptions};
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::theme::{PeekTheme, PeekThemeName};
use crate::viewer::ui::Action;

mod about;
mod animation;
mod content;
mod help;
mod hex;
mod image_render;
mod info;

pub(crate) use about::AboutMode;
pub(crate) use animation::AnimationMode;
pub(crate) use content::ContentMode;
pub(crate) use help::HelpMode;
pub(crate) use hex::HexMode;
pub(crate) use image_render::{ImageKind, ImageRenderMode};
pub(crate) use info::InfoMode;

/// Stable identifier for a mode. Used to look up modes in a stack and
/// to drive view-switch keybindings (e.g. `i` → Info, `x` → Hex).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[allow(dead_code)]
pub(crate) enum ModeId {
    Content,
    Info,
    Help,
    Hex,
    ImageRender,
    Animation,
    StructuredSource,
    About,
}

/// A logical position within the source, expressed in whichever unit the
/// active mode tracks. When switching between modes that track position,
/// the outgoing mode's position is captured here and the incoming mode
/// converts it to its own unit (via `InputSource::byte_to_line` /
/// `line_to_byte` for plain text — extension point for PDF/DOCX, where
/// the mode may consult its own line-to-source-byte map).
///
/// Modes that don't track position (Info, Help, Image, Animation) leave
/// this value untouched on switch, so e.g. Hex → Info → Hex preserves
/// the byte offset.
#[derive(Copy, Clone, Debug)]
pub(crate) enum Position {
    /// No anchor recorded yet (fresh viewer, or no tracking mode has
    /// been active). `set_position(Unknown)` is a no-op.
    Unknown,
    Byte(u64),
    Line(usize),
}

/// Result of `Mode::handle`. `YesResetScroll` indicates that the active
/// mode's scroll offset is no longer meaningful and should be set to 0
/// before the redraw.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Handled {
    No,
    Yes,
    YesResetScroll,
}

impl Handled {
    pub(crate) fn was_consumed(self) -> bool {
        !matches!(self, Self::No)
    }
}

/// Read-only context passed to `Mode::render`.
///
/// `term_cols` is the terminal width in character cells. `term_rows` is
/// the height of the *content area* (terminal rows minus the status line),
/// or `usize::MAX` for non-interactive (pipe) rendering where output is
/// unbounded vertically.
#[allow(dead_code)]
pub(crate) struct RenderCtx<'a> {
    pub source: &'a InputSource,
    pub detected: &'a Detected,
    pub file_info: &'a FileInfo,
    pub theme_name: PeekThemeName,
    pub peek_theme: &'a PeekTheme,
    pub render_opts: RenderOptions,
    pub term_cols: usize,
    pub term_rows: usize,
}

/// One renderable + interactive view of a file.
#[allow(dead_code)]
pub(crate) trait Mode {
    fn id(&self) -> ModeId;
    fn label(&self) -> &str;

    /// Whether this mode is auxiliary — reachable only via dedicated keys
    /// (Tab/i, h, x), not via the `r` primary cycle. Aux modes don't show
    /// up in `cycle_primary`, and toggling their dedicated key returns to
    /// `last_primary`. Default `false`; Info/Help/Hex override.
    fn is_aux(&self) -> bool {
        false
    }

    /// Produce lines for the current viewport. Streaming modes (Hex)
    /// recompute on every call; full-content modes (Info, Help) typically
    /// memoize internally.
    fn render(&mut self, ctx: &RenderCtx) -> Result<Vec<String>>;

    /// True if this mode manages its own scroll position (e.g. Hex's
    /// byte-offset scrolling). When true, `ViewerState`'s line-based
    /// scroll handling is suppressed for this mode.
    fn owns_scroll(&self) -> bool {
        false
    }

    /// Handle a scroll-class action when `owns_scroll` is true. Returns
    /// `true` if the action was consumed (caller re-renders), `false` to
    /// fall through to the shared dispatch.
    fn scroll(&mut self, _action: Action) -> bool {
        false
    }

    /// Whether the rendered output must be regenerated on terminal
    /// resize (e.g. Hex's bytes-per-row depends on terminal width).
    fn rerender_on_resize(&self) -> bool {
        false
    }

    /// Hook called on a terminal resize event before re-rendering.
    /// Modes that maintain layout-dependent state (e.g. Hex's byte-aligned
    /// top offset) update it here. `term_cols` / `term_rows` reflect the
    /// new content area; modes that cache size-derived state should
    /// refresh it from these values.
    fn on_resize(&mut self, _term_cols: usize, _term_rows: usize) {}

    /// Status-line segments contributed by this mode, inserted between
    /// the mode label and the theme-name segment.
    fn status_segments(&self, _theme: &PeekTheme) -> Vec<(String, Color)> {
        Vec::new()
    }

    /// Mode-specific hints prepended to the global hint list on the
    /// right side of the status bar (e.g. Hex's `x:exit hex` when it
    /// was reached from another view). `has_return_target` is true when
    /// pressing the mode's own toggle key would land on a different
    /// (primary) mode — for aux modes like Hex/Info/Help.
    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> {
        Vec::new()
    }

    /// Mode-local actions (in addition to the global set: Quit, scrolling,
    /// theme cycle, mode switching). Used both for key dispatch and the
    /// help screen. Return a `&'static` slice — modes typically expose a
    /// fixed set, even if some are no-ops in some configurations.
    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        &[]
    }

    /// Handle a mode-local action declared in `extra_actions`.
    ///
    /// Returns whether the action was consumed and, if so, whether the
    /// caller should also reset this mode's scroll offset — used when
    /// the action invalidates the meaning of the prior position (e.g.
    /// ContentMode flipping between pretty and raw, where line N maps
    /// to entirely different content).
    fn handle(&mut self, _action: Action) -> Handled {
        Handled::No
    }

    /// How long until this mode wants to be ticked, if at all. The event
    /// loop uses this to drive `event::poll` with a timeout instead of
    /// blocking. `None` (the default) means "block until input arrives".
    fn next_tick(&self) -> Option<Duration> {
        None
    }

    /// Advance internal time-driven state (e.g. animation frame). Called
    /// when `event::poll` times out on the duration returned by
    /// `next_tick`. Return `true` if the mode's content changed and
    /// should be re-rendered.
    fn tick(&mut self) -> bool {
        false
    }

    /// Whether this mode participates in position tracking — i.e. when
    /// switching away its `position()` is captured, and when switching
    /// in its `set_position` is invoked. Default `false` for views like
    /// Info, Help, Image preview, and Animation that have no meaningful
    /// notion of "where in the file" and should pass through any saved
    /// position untouched.
    fn tracks_position(&self) -> bool {
        false
    }

    /// The mode's current logical position within the source. Called on
    /// switch-out for modes that override `tracks_position`. Modes that
    /// own their scroll (Hex) report it directly; line-scrolled modes
    /// (Content) leave this `Unknown` and let `ViewerState` substitute
    /// `Position::Line(scroll_offset)` from its own per-mode scroll.
    fn position(&self) -> Position {
        Position::Unknown
    }

    /// Adjust internal state so the mode lands on `pos`. Called on
    /// switch-in for modes that override `tracks_position`. The mode is
    /// free to consult `source` for unit conversions it can't do alone
    /// (e.g. Hex's `Line → Byte` via `source.line_to_byte`).
    fn set_position(&mut self, _pos: Position, _source: &InputSource) {}

    /// Drain any warnings the mode produced during recent renders.
    /// `ViewerState` polls this after each render and merges the result
    /// into `FileInfo.warnings` so InfoMode picks them up. Default empty.
    fn take_warnings(&mut self) -> Vec<String> {
        Vec::new()
    }
}
