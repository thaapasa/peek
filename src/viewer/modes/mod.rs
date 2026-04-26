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

use crate::info::FileInfo;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::theme::{PeekTheme, PeekThemeName};
use crate::viewer::ui::Action;

mod animation;
mod content;
mod help;
mod hex;
mod image_render;
mod info;

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
    StructuredSource,
}

/// Read-only context passed to `Mode::render`.
#[allow(dead_code)]
pub(crate) struct RenderCtx<'a> {
    pub source: &'a InputSource,
    pub detected: &'a Detected,
    pub file_info: &'a FileInfo,
    pub theme_name: PeekThemeName,
    pub peek_theme: &'a PeekTheme,
}

/// One renderable + interactive view of a file.
#[allow(dead_code)]
pub(crate) trait Mode {
    fn id(&self) -> ModeId;
    fn label(&self) -> &str;

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
    /// top offset) update it here.
    fn on_resize(&mut self) {}

    /// Status-line segments contributed by this mode, inserted between
    /// the mode label and the theme-name segment.
    fn status_segments(&self, _theme: &PeekTheme) -> Vec<(String, Color)> {
        Vec::new()
    }

    /// Mode-local actions (in addition to the global set: Quit, scrolling,
    /// theme cycle, mode switching). Used both for key dispatch and the
    /// help screen. Return a `&'static` slice — modes typically expose a
    /// fixed set, even if some are no-ops in some configurations.
    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        &[]
    }

    /// Handle a mode-local action declared in `extra_actions`. Return
    /// `true` if the action was consumed (caller invalidates the cached
    /// lines and re-renders).
    fn handle(&mut self, _action: Action) -> bool {
        false
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
}
