use std::io::{self, Write};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    cursor,
    event::KeyEvent,
    execute,
    terminal::{self, ClearType},
};

use crate::info::{FileInfo, RenderOptions};
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::theme::{ColorMode, PeekTheme, PeekThemeName};
use crate::viewer::modes::{Handled, Mode, ModeId, Position, RenderCtx};

use super::keys::{self, Action, Outcome};
use super::{content_rows, make_peek_theme, terminal_cols};

/// One mode's most recent windowed render. The `lines` field is the
/// exact slice that should be drawn at the top of the viewport; the
/// `scroll_at` and `rows_at` fields are the inputs the mode was given,
/// used as the cache key. `total` is the full-source line count so
/// scroll math (max_scroll, Bottom jump) doesn't need to re-render.
struct RenderedView {
    lines: Vec<String>,
    scroll_at: usize,
    rows_at: usize,
    total: usize,
}

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
    (Action::CycleView, "Cycle file's view modes"),
    (Action::SwitchInfo, "File info"),
    (Action::ToggleHelp, "Toggle help"),
    (Action::SwitchToHex, "Hex dump mode"),
    (Action::SwitchToAbout, "About / status screen"),
    (Action::CycleTheme, "Next theme"),
    (Action::CycleColorMode, "Next color mode"),
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
    /// Per-mode rendered window cache. `None` = needs render (lazy on
    /// first use, invalidated by scroll change, theme/color cycle, or
    /// resize for modes that opt in via `rerender_on_resize`).
    views: Vec<Option<RenderedView>>,

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
        color_mode: ColorMode,
        render_opts: RenderOptions,
        modes: Vec<Box<dyn Mode>>,
    ) -> Result<Self> {
        assert!(!modes.is_empty(), "ViewerState needs at least one mode");
        let n = modes.len();
        let peek_theme = make_peek_theme(theme_name, color_mode);
        let file_info = crate::info::gather(source, detected)?;
        let last_primary = if modes[0].is_aux() { None } else { Some(0) };
        let mut views = Vec::with_capacity(n);
        for _ in 0..n {
            views.push(None);
        }
        Ok(Self {
            modes,
            active: 0,
            last_primary,
            scroll: vec![0; n],
            views,
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

    pub(crate) fn apply(&mut self, action: Action) -> Result<Outcome> {
        Ok(match action {
            Action::Quit => Outcome::Quit,
            Action::ScrollUp => {
                self.scroll_by(-1)?;
                Outcome::Redraw
            }
            Action::ScrollDown => {
                self.scroll_by(1)?;
                Outcome::Redraw
            }
            Action::PageUp => {
                self.page(-1)?;
                Outcome::Redraw
            }
            Action::PageDown => {
                self.page(1)?;
                Outcome::Redraw
            }
            Action::Top => {
                self.scroll[self.active] = 0;
                Outcome::Redraw
            }
            Action::Bottom => {
                self.prepare_total()?;
                self.scroll[self.active] = self.max_scroll();
                Outcome::Redraw
            }
            Action::SwitchInfo => {
                self.jump_to(ModeId::Info);
                Outcome::Redraw
            }
            Action::CycleView => {
                self.cycle_view();
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
            Action::SwitchToAbout => {
                self.toggle_aux(ModeId::About);
                Outcome::Redraw
            }
            Action::CycleTheme => {
                self.cycle_theme();
                Outcome::Redraw
            }
            Action::CycleColorMode => {
                self.cycle_color_mode();
                Outcome::Redraw
            }
            // Mode-local actions: routed via the mode's own `handle` before
            // we get here. Listed explicitly so adding a new Action variant
            // forces a non-exhaustive-match compile error in this function
            // and a deliberate decision about which side handles it.
            Action::ToggleRawSource
            | Action::PlayPause
            | Action::NextFrame
            | Action::PrevFrame
            | Action::CycleBackground
            | Action::CycleImageMode
            | Action::CycleFitMode
            | Action::ScrollLeft
            | Action::ScrollRight
            | Action::ToggleLineNumbers => Outcome::Unhandled,
        })
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

    /// Advance the active index to the next view mode in the cycle.
    /// Bound to `Tab`. Walks every mode except the overlay-style aux
    /// modes (Help, About) and Hex — Hex has its own dedicated key and
    /// is not part of the document-view cycle. The exception is binary
    /// files, where Hex *is* the data view: when no non-aux mode exists,
    /// Hex is included so Tab still toggles Hex ↔ Info.
    fn cycle_view(&mut self) {
        let n = self.modes.len();
        let has_primary = self.modes.iter().any(|m| !m.is_aux());
        let mut i = self.active;
        for _ in 0..n {
            i = (i + 1) % n;
            if i == self.active {
                break;
            }
            let id = self.modes[i].id();
            if matches!(id, ModeId::Help | ModeId::About) {
                continue;
            }
            if id == ModeId::Hex && has_primary {
                continue;
            }
            self.set_active(i);
            return;
        }
    }

    fn cycle_theme(&mut self) {
        self.current_theme = self.current_theme.next();
        self.peek_theme = make_peek_theme(self.current_theme, self.peek_theme.color_mode);
        // All themed views are stale.
        for slot in &mut self.views {
            *slot = None;
        }
    }

    fn cycle_color_mode(&mut self) {
        self.peek_theme.color_mode = self.peek_theme.color_mode.next();
        // Every cached line embeds escape sequences keyed to the previous
        // mode — invalidate them all so the next draw re-paints in the new
        // encoding.
        for slot in &mut self.views {
            *slot = None;
        }
    }

    // ---------------------------------------------------------------------
    // Resize
    // ---------------------------------------------------------------------

    pub(crate) fn handle_resize(&mut self) {
        let cols = terminal_cols();
        let rows = content_rows();
        for (i, m) in self.modes.iter_mut().enumerate() {
            m.on_resize(cols, rows);
            if m.rerender_on_resize() {
                self.views[i] = None;
            }
        }
    }

    // ---------------------------------------------------------------------
    // Rendering
    // ---------------------------------------------------------------------

    /// Ensure the active mode's view is rendered for the current scroll
    /// position and viewport height. A cached view from a previous render
    /// is reused only when its scroll_at and rows_at match the current
    /// request; any scroll change forces a re-render so streaming modes
    /// (ContentMode) can fetch the new window.
    pub(crate) fn ensure_active_rendered(&mut self) -> Result<()> {
        let scroll = self.scroll[self.active];
        let rows = content_rows();
        let cache_hit = self.views[self.active]
            .as_ref()
            .is_some_and(|v| v.scroll_at == scroll && v.rows_at == rows);
        if !cache_hit {
            let view = self.render_active()?;
            self.views[self.active] = Some(view);
        }
        Ok(())
    }

    /// Mark the active mode's view as stale so the next draw re-renders.
    pub(crate) fn invalidate_active(&mut self) {
        self.views[self.active] = None;
    }

    fn render_active(&mut self) -> Result<RenderedView> {
        let scroll = self.scroll[self.active];
        let rows = content_rows();
        let ctx = RenderCtx {
            source: self.source,
            detected: self.detected,
            file_info: &self.file_info,
            theme_name: self.current_theme,
            peek_theme: &self.peek_theme,
            render_opts: self.render_opts,
            term_cols: terminal_cols(),
            term_rows: rows,
        };
        let window = self.modes[self.active].render_window(&ctx, scroll, rows)?;
        // Drain any warnings the mode raised during render (e.g. ContentMode's
        // lazy pretty-print failure) and merge into FileInfo so InfoMode
        // surfaces them. Invalidate Info's cache when new warnings arrived
        // so it re-renders with the updated list.
        let new_warnings = self.modes[self.active].take_warnings();
        if !new_warnings.is_empty() {
            self.file_info.warnings.extend(new_warnings);
            if let Some(idx) = self.mode_index(ModeId::Info) {
                self.views[idx] = None;
            }
        }
        Ok(RenderedView {
            lines: window.lines,
            scroll_at: scroll,
            rows_at: rows,
            total: window.total,
        })
    }

    fn current_lines(&self) -> &[String] {
        self.views[self.active]
            .as_ref()
            .map(|v| v.lines.as_slice())
            .unwrap_or(&[])
    }

    // ---------------------------------------------------------------------
    // Line scrolling (used when active mode does NOT own scroll)
    // ---------------------------------------------------------------------

    /// Maximum scroll offset for the active mode's last-rendered total.
    /// Falls back to 0 when no render has happened yet — callers that
    /// need an authoritative total (Bottom action) should ensure a
    /// render first via `prepare_total`.
    fn max_scroll(&self) -> usize {
        let total = self.views[self.active].as_ref().map_or(0, |v| v.total);
        total.saturating_sub(content_rows())
    }

    /// Ensure the active mode's `total` is known. Prefers a cheap
    /// `Mode::total_lines()` (ContentMode answers in O(1) from its
    /// LineSource); falls back to a render. Used by Bottom-jumps where
    /// we need the line count before adjusting scroll.
    fn prepare_total(&mut self) -> Result<()> {
        if let Some(n) = self.modes[self.active].total_lines() {
            // Seed a placeholder view so max_scroll has something to read
            // without forcing an early render. Real lines arrive on the
            // next draw.
            let needs_seed = self.views[self.active]
                .as_ref()
                .is_none_or(|v| v.total != n);
            if needs_seed {
                self.views[self.active] = Some(RenderedView {
                    lines: Vec::new(),
                    scroll_at: usize::MAX, // force re-render on next draw
                    rows_at: content_rows(),
                    total: n,
                });
            }
            return Ok(());
        }
        if self.views[self.active].is_none() {
            self.ensure_active_rendered()?;
        }
        Ok(())
    }

    fn scroll_by(&mut self, delta: isize) -> Result<()> {
        if self.modes[self.active].owns_scroll() {
            return Ok(());
        }
        self.prepare_total()?;
        let max = self.max_scroll();
        let s = &mut self.scroll[self.active];
        *s = if delta < 0 {
            s.saturating_sub((-delta) as usize)
        } else {
            (*s + delta as usize).min(max)
        };
        Ok(())
    }

    fn page(&mut self, direction: isize) -> Result<()> {
        if self.modes[self.active].owns_scroll() {
            return Ok(());
        }
        self.prepare_total()?;
        let step = content_rows().saturating_sub(1);
        let max = self.max_scroll();
        let s = &mut self.scroll[self.active];
        *s = if direction < 0 {
            s.saturating_sub(step)
        } else {
            (*s + step).min(max)
        };
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Drawing
    // ---------------------------------------------------------------------

    pub(crate) fn draw(&self, stdout: &mut io::Stdout, status: &str) -> Result<()> {
        draw_screen(
            stdout,
            self.current_lines(),
            status,
            self.peek_theme.color_mode.reset_bytes(),
        )
    }
}

/// Render the screen: clear, draw the pre-windowed `lines` (already
/// sliced by the active mode for the current scroll), draw status bar
/// on last row.
fn draw_screen(
    stdout: &mut io::Stdout,
    lines: &[String],
    status: &str,
    reset_bytes: &[u8],
) -> Result<()> {
    let (_cols, total_rows) = terminal::size().unwrap_or((80, 24));
    let rows = (total_rows as usize).saturating_sub(1);

    // Reset all attributes before clearing so the clear doesn't fill the
    // screen with a leftover background color. (Empty in Plain mode —
    // there's nothing to reset.)
    stdout.write_all(reset_bytes)?;
    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0),
    )?;

    let end = lines.len().min(rows);
    for (i, line) in lines[..end].iter().enumerate() {
        if i > 0 {
            stdout.write_all(b"\r\n")?;
        }
        stdout.write_all(line.as_bytes())?;
    }

    // Reset all attributes, then draw the status line on the last row.
    stdout.write_all(reset_bytes)?;
    execute!(stdout, cursor::MoveTo(0, total_rows.saturating_sub(1)))?;
    stdout.write_all(status.as_bytes())?;

    stdout.flush()?;
    Ok(())
}
