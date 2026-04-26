use std::io::{self, Write};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    cursor, event::KeyEvent, execute,
    terminal::{self, ClearType},
};

use crate::info::{FileInfo, RenderOptions};
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::theme::{ANSI_RESET_BYTES, PeekTheme, PeekThemeName};
use crate::viewer::modes::{Handled, Mode, ModeId, Position, RenderCtx};

use super::keys::{self, Action, Outcome};
use super::{content_rows, make_peek_theme};

/// Global actions that work in every mode (unless the mode shadows the
/// key via its own `extra_actions`). Used for both key dispatch and the
/// help screen.
pub(crate) const GLOBAL_ACTIONS: &[(Action, &str)] = &[
    (Action::Quit, "Quit"),
    (Action::ScrollUp, "Scroll up"),
    (Action::ScrollDown, "Scroll down"),
    (Action::PageUp, "Page up"),
    (Action::PageDown, "Page down"),
    (Action::Top, "Jump to top"),
    (Action::Bottom, "Jump to bottom"),
    (Action::ToggleContentInfo, "Toggle content / file info"),
    (Action::SwitchInfo, "File info"),
    (Action::ToggleHelp, "Toggle help"),
    (Action::SwitchToHex, "Hex dump mode"),
    (Action::CycleTheme, "Next theme"),
    // `r` is dispatched globally so modes that don't handle it locally
    // fall through to `cycle_primary` (e.g. SVG rasterized → XML view).
    (Action::ToggleRawSource, "Toggle raw / pretty / cycle primary"),
];


pub(crate) struct ViewerState<'a> {
    modes: Vec<Box<dyn Mode>>,
    active: usize,
    /// The most recent primary (non-aux) mode the user was on. Aux modes
    /// (Info, Help, Hex) toggle back here when their dedicated key is
    /// pressed again. Aux-to-aux transitions don't update this slot, so
    /// the path back to "your actual work" survives any number of detours
    /// (Hex → Info → Help → Tab still returns to the original primary).
    /// `None` only when the stack contains no primary modes (binary
    /// files: Hex is the default and toggling out is a no-op).
    last_primary: Option<usize>,
    scroll: Vec<usize>,
    /// Per-mode rendered lines. `None` = needs render (lazy on first use,
    /// invalidated by theme cycles and resize).
    lines: Vec<Option<Vec<String>>>,

    /// Last known logical position in the source. Captured from any
    /// position-tracking mode on switch-out and pushed to the next one
    /// on switch-in. Modes that opt out (Info/Help/Image/Animation)
    /// pass it through unchanged.
    position: Position,

    pub current_theme: PeekThemeName,
    pub peek_theme: PeekTheme,

    source: &'a InputSource,
    detected: &'a Detected,
    file_info: FileInfo,
    render_opts: RenderOptions,
}

impl<'a> ViewerState<'a> {
    pub(crate) fn new(
        source: &'a InputSource,
        detected: &'a Detected,
        theme_name: PeekThemeName,
        render_opts: RenderOptions,
        modes: Vec<Box<dyn Mode>>,
    ) -> Result<Self> {
        assert!(!modes.is_empty(), "ViewerState needs at least one mode");
        let n = modes.len();
        let peek_theme = make_peek_theme(theme_name);
        let file_info = crate::info::gather(source, detected)?;
        let last_primary = if modes[0].is_aux() { None } else { Some(0) };
        Ok(Self {
            modes,
            active: 0,
            last_primary,
            scroll: vec![0; n],
            lines: vec![None; n],
            position: Position::Unknown,
            current_theme: theme_name,
            peek_theme,
            source,
            detected,
            file_info,
            render_opts,
        })
    }

    // ---------------------------------------------------------------------
    // Active mode access
    // ---------------------------------------------------------------------

    pub(crate) fn active_label(&self) -> &str {
        self.modes[self.active].label()
    }

    pub(crate) fn active_status_segments(&self) -> Vec<(String, syntect::highlighting::Color)> {
        self.modes[self.active].status_segments(&self.peek_theme)
    }

    /// Mode-contributed hint strings for the status bar (right side).
    /// The active mode is asked whether it has anywhere to return to,
    /// so e.g. Hex can show `x:exit hex` only when toggling out lands
    /// elsewhere.
    pub(crate) fn active_status_hints(&self) -> Vec<&'static str> {
        self.modes[self.active].status_hints(self.has_return_target())
    }

    /// Whether the active aux mode has somewhere meaningful to return to
    /// when toggled off (used to decide whether to show "x:exit hex" etc.
    /// in the status line).
    pub(crate) fn has_return_target(&self) -> bool {
        self.last_primary.is_some_and(|i| i != self.active)
    }

    fn mode_index(&self, id: ModeId) -> Option<usize> {
        self.modes.iter().position(|m| m.id() == id)
    }

    // ---------------------------------------------------------------------
    // Key dispatch
    // ---------------------------------------------------------------------

    /// Resolve a key event to an `Action`. Globals always win on conflict
    /// so a mode's extras can never accidentally shadow `Quit`, scrolling,
    /// theme cycle, etc. (No mode shadows today, but the order makes the
    /// invariant structural.)
    pub(crate) fn dispatch_key(&self, key: KeyEvent) -> Option<Action> {
        let extras = self.modes[self.active].extra_actions();
        keys::dispatch(key, GLOBAL_ACTIONS).or_else(|| keys::dispatch(key, extras))
    }

    /// Try to consume the action via the active mode's scroll handler.
    /// Returns `true` if consumed (caller invalidates and re-renders).
    pub(crate) fn try_active_scroll(&mut self, action: Action) -> bool {
        let m = &mut self.modes[self.active];
        if !m.owns_scroll() {
            return false;
        }
        m.scroll(action)
    }

    /// Try to consume the action via the active mode's `handle()`.
    /// Returns whether the action was consumed; on `YesResetScroll`,
    /// the active mode's scroll offset is also zeroed.
    pub(crate) fn try_active_handle(&mut self, action: Action) -> bool {
        let handled = self.modes[self.active].handle(action);
        if handled == Handled::YesResetScroll {
            self.scroll[self.active] = 0;
        }
        handled.was_consumed()
    }

    /// Active mode's preferred next-tick deadline, if any. Drives the
    /// event-loop timeout for animation-style modes.
    pub(crate) fn active_next_tick(&self) -> Option<Duration> {
        self.modes[self.active].next_tick()
    }

    /// Tick the active mode; returns `true` when its content changed.
    pub(crate) fn tick_active(&mut self) -> bool {
        self.modes[self.active].tick()
    }

    // ---------------------------------------------------------------------
    // Globals — apply() handles everything not consumed by the active mode
    // ---------------------------------------------------------------------

    pub(crate) fn apply(&mut self, action: Action) -> Outcome {
        match action {
            Action::Quit => Outcome::Quit,
            Action::ScrollUp => {
                self.scroll_by(-1);
                Outcome::Redraw
            }
            Action::ScrollDown => {
                self.scroll_by(1);
                Outcome::Redraw
            }
            Action::PageUp => {
                self.page(-1);
                Outcome::Redraw
            }
            Action::PageDown => {
                self.page(1);
                Outcome::Redraw
            }
            Action::Top => {
                self.scroll[self.active] = 0;
                Outcome::Redraw
            }
            Action::Bottom => {
                self.scroll[self.active] = self.max_scroll();
                Outcome::Redraw
            }
            Action::SwitchInfo => {
                self.jump_to(ModeId::Info);
                Outcome::Redraw
            }
            Action::ToggleContentInfo => {
                self.toggle_aux(ModeId::Info);
                Outcome::Redraw
            }
            Action::ToggleHelp => {
                self.toggle_aux(ModeId::Help);
                Outcome::Redraw
            }
            Action::SwitchToHex => {
                self.toggle_aux(ModeId::Hex);
                Outcome::Redraw
            }
            Action::CycleTheme => {
                self.cycle_theme();
                Outcome::Redraw
            }
            Action::ToggleRawSource => {
                // Active mode declined `r` — cycle to the next primary mode
                // (skipping Info/Help/Hex). For SVG, this swaps rasterized
                // ↔ XML view; for files with one primary, no-op.
                self.cycle_primary();
                Outcome::Redraw
            }
            // Mode-local actions: routed via the mode's own `handle` before
            // we get here. Listed explicitly so adding a new Action variant
            // forces a non-exhaustive-match compile error in this function
            // and a deliberate decision about which side handles it.
            Action::PlayPause
            | Action::NextFrame
            | Action::PrevFrame
            | Action::CycleBackground
            | Action::CycleImageMode => Outcome::Unhandled,
        }
    }

    // ---------------------------------------------------------------------
    // Mode switching helpers
    // ---------------------------------------------------------------------

    /// One-way jump used by `i` (SwitchInfo): land on the target if it
    /// exists in the stack. No back-link logic — pressing `i` from Info
    /// is a no-op.
    fn jump_to(&mut self, target: ModeId) {
        if let Some(idx) = self.mode_index(target) {
            self.set_active(idx);
        }
    }

    /// Aux toggle shared by Tab (→ Info), `h` (→ Help), and `x` (→ Hex).
    /// On the target aux mode, return to `last_primary` (the user's
    /// "actual work"). Otherwise, enter the target aux. Aux is "hidden":
    /// it doesn't show up in Tab cycling and you can only land on it via
    /// its dedicated key.
    fn toggle_aux(&mut self, target: ModeId) {
        if self.modes[self.active].id() == target {
            // Exit aux. Return to the last primary, or fall back to mode
            // 0 (which is Hex itself for binary files — pressing `x`
            // there is then a no-op, matching the old standalone-hex UX).
            let dest = self.last_primary.unwrap_or(0);
            if dest != self.active {
                self.set_active(dest);
            }
        } else if let Some(idx) = self.mode_index(target) {
            self.set_active(idx);
        }
        // Target not in this stack → no-op.
    }

    /// Switch the active mode index. Updates `last_primary` whenever we
    /// land on a non-aux mode, captures the outgoing mode's position (if
    /// it tracks) and restores it on the incoming mode (if it tracks).
    /// Modes that don't track position leave `self.position` untouched —
    /// Hex → Info → Hex preserves the byte offset across the detour.
    fn set_active(&mut self, new_idx: usize) {
        if new_idx == self.active {
            return;
        }
        self.capture_position();
        self.active = new_idx;
        if !self.modes[new_idx].is_aux() {
            self.last_primary = Some(new_idx);
        }
        self.restore_position();
    }

    fn capture_position(&mut self) {
        let mode = &self.modes[self.active];
        if !mode.tracks_position() {
            return;
        }
        let pos = if mode.owns_scroll() {
            mode.position()
        } else {
            // Line-scrolled mode: the top visible line is the position.
            Position::Line(self.scroll[self.active])
        };
        if !matches!(pos, Position::Unknown) {
            self.position = pos;
        }
    }

    fn restore_position(&mut self) {
        let mode = &mut self.modes[self.active];
        if !mode.tracks_position() {
            return;
        }
        if mode.owns_scroll() {
            mode.set_position(self.position, self.source);
            return;
        }
        // Line-scrolled mode: convert to line and seed scroll[active].
        let line = match self.position {
            Position::Line(l) => Some(l),
            Position::Byte(b) => self.source.byte_to_line(b),
            Position::Unknown => None,
        };
        if let Some(l) = line {
            self.scroll[self.active] = l;
        }
    }

    /// Advance the active index to the next primary (non-aux) mode.
    /// For SVG this swaps rasterized ↔ XML; for files with a single
    /// primary mode, this is a no-op. Used as the fallback handler for
    /// `r` when no mode consumes it.
    fn cycle_primary(&mut self) {
        let n = self.modes.len();
        let mut i = self.active;
        for _ in 0..n {
            i = (i + 1) % n;
            if i == self.active {
                break;
            }
            if !self.modes[i].is_aux() {
                self.set_active(i);
                return;
            }
        }
    }

    fn cycle_theme(&mut self) {
        self.current_theme = self.current_theme.next();
        self.peek_theme = make_peek_theme(self.current_theme);
        // All themed lines are stale.
        for slot in &mut self.lines {
            *slot = None;
        }
    }

    // ---------------------------------------------------------------------
    // Resize
    // ---------------------------------------------------------------------

    pub(crate) fn handle_resize(&mut self) {
        for (i, m) in self.modes.iter_mut().enumerate() {
            m.on_resize();
            if m.rerender_on_resize() {
                self.lines[i] = None;
            }
        }
    }

    // ---------------------------------------------------------------------
    // Rendering
    // ---------------------------------------------------------------------

    /// Ensure the active mode's lines are rendered (lazy). Called before
    /// each draw.
    pub(crate) fn ensure_active_rendered(&mut self) -> Result<()> {
        if self.lines[self.active].is_none() {
            let lines = self.render_active()?;
            self.lines[self.active] = Some(lines);
        }
        Ok(())
    }

    /// Mark the active mode's lines as stale so the next draw re-renders.
    pub(crate) fn invalidate_active(&mut self) {
        self.lines[self.active] = None;
    }

    fn render_active(&mut self) -> Result<Vec<String>> {
        let ctx = RenderCtx {
            source: self.source,
            detected: self.detected,
            file_info: &self.file_info,
            theme_name: self.current_theme,
            peek_theme: &self.peek_theme,
            render_opts: self.render_opts,
        };
        let lines = self.modes[self.active].render(&ctx)?;
        // Drain any warnings the mode raised during render (e.g. ContentMode's
        // lazy pretty-print failure) and merge into FileInfo so InfoMode
        // surfaces them. Invalidate Info's cache when new warnings arrived
        // so it re-renders with the updated list.
        let new_warnings = self.modes[self.active].take_warnings();
        if !new_warnings.is_empty() {
            self.file_info.warnings.extend(new_warnings);
            if let Some(idx) = self.mode_index(ModeId::Info) {
                self.lines[idx] = None;
            }
        }
        Ok(lines)
    }

    fn current_lines(&self) -> &[String] {
        self.lines[self.active].as_deref().unwrap_or(&[])
    }

    fn current_scroll(&self) -> usize {
        self.scroll[self.active]
    }

    // ---------------------------------------------------------------------
    // Line scrolling (used when active mode does NOT own scroll)
    // ---------------------------------------------------------------------

    fn max_scroll(&self) -> usize {
        let len = self.lines[self.active].as_deref().map_or(0, |l| l.len());
        len.saturating_sub(content_rows())
    }

    fn scroll_by(&mut self, delta: isize) {
        if self.modes[self.active].owns_scroll() {
            return;
        }
        let max = self.max_scroll();
        let s = &mut self.scroll[self.active];
        *s = if delta < 0 {
            s.saturating_sub((-delta) as usize)
        } else {
            (*s + delta as usize).min(max)
        };
    }

    fn page(&mut self, direction: isize) {
        if self.modes[self.active].owns_scroll() {
            return;
        }
        let step = content_rows().saturating_sub(1);
        let max = self.max_scroll();
        let s = &mut self.scroll[self.active];
        *s = if direction < 0 {
            s.saturating_sub(step)
        } else {
            (*s + step).min(max)
        };
    }

    // ---------------------------------------------------------------------
    // Drawing
    // ---------------------------------------------------------------------

    pub(crate) fn draw(&self, stdout: &mut io::Stdout, status: &str) -> Result<()> {
        draw_screen(stdout, self.current_lines(), self.current_scroll(), status)
    }

}

/// Render the screen: clear, draw visible lines, draw status bar on last row.
fn draw_screen(
    stdout: &mut io::Stdout,
    lines: &[String],
    scroll: usize,
    status: &str,
) -> Result<()> {
    let (_cols, total_rows) = terminal::size().unwrap_or((80, 24));
    let rows = (total_rows as usize).saturating_sub(1);

    // Reset all attributes before clearing so the clear doesn't fill the
    // screen with a leftover background color.
    stdout.write_all(ANSI_RESET_BYTES)?;
    execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0),)?;

    let start = scroll.min(lines.len());
    let end = (start + rows).min(lines.len());
    for (i, line) in lines[start..end].iter().enumerate() {
        if i > 0 {
            stdout.write_all(b"\r\n")?;
        }
        stdout.write_all(line.as_bytes())?;
    }

    // Reset all attributes, then draw the status line on the last row.
    stdout.write_all(ANSI_RESET_BYTES)?;
    execute!(stdout, cursor::MoveTo(0, total_rows.saturating_sub(1)))?;
    stdout.write_all(status.as_bytes())?;

    stdout.flush()?;
    Ok(())
}
