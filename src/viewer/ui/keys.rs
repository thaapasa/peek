use crossterm::event::{KeyCode, KeyEvent};

pub(crate) enum KeyAction {
    /// User wants to quit (q, Esc, Ctrl+C).
    Quit,
    /// View mode or scroll changed; caller should redraw.
    Redraw,
    /// Theme was cycled; caller must re-render content_lines, then redraw.
    ThemeChanged,
    /// User pressed `x` — caller should switch into (or out of) hex mode.
    SwitchToHex,
    /// Key not handled; caller should check viewer-specific bindings.
    Unhandled(KeyEvent),
}

/// Shared `b` binding for cycling the image-viewer background. Storage is
/// viewer-specific (Cell in `interactive.rs`, owned field in `animate.rs`),
/// so this only collapses the key-name knowledge.
pub(crate) fn is_background_cycle(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('b'))
}
