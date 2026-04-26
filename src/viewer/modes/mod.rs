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

use anyhow::Result;
use syntect::highlighting::Color;

use crate::info::FileInfo;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::theme::{PeekTheme, PeekThemeName};
use crate::viewer::ui::Action;

mod help;
mod hex;
mod info;

pub(crate) use help::HelpMode;
pub(crate) use hex::HexMode;
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

    /// Status-line segments contributed by this mode, inserted between
    /// the mode label and the theme-name segment.
    fn status_segments(&self, _theme: &PeekTheme) -> Vec<(String, Color)> {
        Vec::new()
    }
}
