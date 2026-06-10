//! `ViewerState` — the interactive session's hub: the recursive-peek
//! frame stack, cross-session state (theme, screen buffer, prompt slot,
//! flash), key dispatch into the active mode, the `apply` handler for
//! session-level actions, and mode switching (jump / aux toggle / view
//! cycling). The sibling modules carry the rest: `frame` (the stack +
//! descend / extract), `prompt` (modal-prompt plumbing), `render` (view
//! cache + failure recovery + scroll math + draw).

use std::time::Duration;

use anyhow::Result;
use crossterm::event::KeyEvent;

use peek_detect::Detected;
use peek_foundation::info::RenderOptions;
use peek_foundation::viewer::modes::{Handled, Mode, ModeId, Position};
use peek_io::InputSource;
use peek_theme::{PeekTheme, PeekThemeName, StyleMode};

use peek_foundation::viewer::ui::keys::{self, Action, Outcome};
use peek_foundation::viewer::ui::make_peek_theme;
use peek_foundation::viewer::ui::prompt::Prompt;
use peek_foundation::viewer::ui::screen::ScreenBuffer;

use super::frame::{SessionFrame, capture_position, restore_position};
use super::prompt::PromptKind;

/// Builds the mode stack for a freshly-pushed session. Captured at
/// `ViewerState` construction so `descend` doesn't have to know about
/// `Registry` / `Args`.
pub(crate) type ModeBuilder = Box<dyn Fn(&InputSource, &Detected) -> Result<Vec<Box<dyn Mode>>>>;

pub(crate) struct ViewerState {
    /// Recursive-peek stack. Always non-empty while the viewer runs;
    /// the last `Back` on a single-frame stack returns `Outcome::Quit`.
    pub(super) frames: Vec<SessionFrame>,

    /// Builds the mode stack for a freshly-pushed session. See
    /// [`ModeBuilder`].
    pub(super) mode_builder: ModeBuilder,

    pub current_theme: PeekThemeName,
    pub peek_theme: PeekTheme,

    /// Frame buffer: caches the previous draw, skips writes for
    /// unchanged rows. Invalidated on resize and on stack push/pop.
    pub(super) screen: ScreenBuffer,
    pub(super) render_opts: RenderOptions,

    /// Modal prompt overlay. While `Some`, raw key events go to the
    /// prompt and the status line shows its render. The paired
    /// [`PromptKind`] is the work to run on confirm — keeps the Prompt
    /// widget oblivious to its purpose.
    pub(super) prompt: Option<(Prompt, PromptKind)>,

    /// One-shot status flash (e.g. "wrote /tmp/foo"). Cleared after
    /// one redraw.
    pub(super) flash: Option<String>,

    /// Mirror of the CLI `--no-tempfile` flag. Threaded into every
    /// `ExtractOptions` the interactive viewer builds so user choice
    /// persists across descend / extract presses.
    pub(super) no_tempfile: bool,
}

impl ViewerState {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        source: InputSource,
        detected: Detected,
        theme_name: PeekThemeName,
        style_mode: StyleMode,
        render_opts: RenderOptions,
        modes: Vec<Box<dyn Mode>>,
        mode_builder: ModeBuilder,
        no_tempfile: bool,
    ) -> Result<Self> {
        let peek_theme = make_peek_theme(theme_name, style_mode);
        let file_info = crate::gather::gather(&source, &detected)?;
        let frame = SessionFrame::new(source, detected, file_info, modes);
        Ok(Self {
            frames: vec![frame],
            mode_builder,
            current_theme: theme_name,
            peek_theme,
            screen: ScreenBuffer::new(),
            render_opts,
            prompt: None,
            flash: None,
            no_tempfile,
        })
    }

    pub(crate) fn frame(&self) -> &SessionFrame {
        self.frames.last().expect("non-empty stack")
    }

    pub(super) fn frame_mut(&mut self) -> &mut SessionFrame {
        self.frames.last_mut().expect("non-empty stack")
    }

    #[cfg(test)]
    pub(crate) fn stack_depth(&self) -> usize {
        self.frames.len()
    }

    // ---------------------------------------------------------------------
    // Active mode access
    // ---------------------------------------------------------------------

    pub(crate) fn active_label(&self) -> &str {
        let f = self.frame();
        f.modes[f.active].label()
    }

    pub(crate) fn active_status_segments(&self) -> Vec<(String, syntect::highlighting::Color)> {
        let f = self.frame();
        f.modes[f.active].status_segments(&self.peek_theme)
    }

    pub(crate) fn active_status_hints(&self) -> Vec<&'static str> {
        let has_return = self.has_return_target();
        let f = self.frame();
        f.modes[f.active].status_hints(has_return)
    }

    pub(crate) fn has_return_target(&self) -> bool {
        let f = self.frame();
        f.last_primary.is_some_and(|i| i != f.active)
    }

    // ---------------------------------------------------------------------
    // Key dispatch
    // ---------------------------------------------------------------------

    pub(crate) fn dispatch_key(&self, key: KeyEvent) -> Option<Action> {
        let f = self.frame();
        let extras = f.modes[f.active].extra_actions();
        keys::dispatch(key, keys::GLOBAL_ACTIONS).or_else(|| keys::dispatch(key, extras))
    }

    pub(crate) fn try_active_scroll(&mut self, action: Action) -> bool {
        let f = self.frame_mut();
        let active = f.active;
        let m = &mut f.modes[active];
        if !m.owns_scroll() {
            return false;
        }
        m.scroll(action)
    }

    pub(crate) fn try_active_handle(&mut self, action: Action) -> bool {
        let f = self.frame_mut();
        let active = f.active;
        let handled = f.modes[active].handle(action);
        match handled {
            Handled::YesResetScroll => f.scroll[active] = 0,
            Handled::YesScrollTo(n) => f.scroll[active] = n,
            Handled::No | Handled::Yes => {}
        }
        handled.was_consumed()
    }

    pub(crate) fn active_next_tick(&self) -> Option<Duration> {
        let f = self.frame();
        f.modes[f.active].next_tick()
    }

    pub(crate) fn tick_active(&mut self) -> bool {
        let f = self.frame_mut();
        let active = f.active;
        f.modes[active].tick()
    }

    // ---------------------------------------------------------------------
    // Globals — apply() handles everything not consumed by the active mode
    // ---------------------------------------------------------------------

    pub(crate) fn apply(&mut self, action: Action) -> Result<Outcome> {
        Ok(match action {
            Action::Quit => Outcome::Quit,
            Action::Back => {
                if self.frames.len() > 1 {
                    self.pop_frame();
                    Outcome::Redraw
                } else {
                    Outcome::Quit
                }
            }
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
                let f = self.frame_mut();
                f.scroll[f.active] = 0;
                Outcome::Redraw
            }
            Action::Bottom => {
                self.prepare_total()?;
                let max = self.max_scroll();
                let f = self.frame_mut();
                f.scroll[f.active] = max;
                Outcome::Redraw
            }
            Action::SwitchInfo => {
                self.jump_to(ModeId::Info);
                Outcome::Redraw
            }
            Action::CycleView => {
                self.cycle_view(1);
                Outcome::Redraw
            }
            Action::CycleViewBack => {
                self.cycle_view(-1);
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
                self.cycle_theme(1);
                Outcome::Redraw
            }
            Action::CycleThemeBack => {
                self.cycle_theme(-1);
                Outcome::Redraw
            }
            Action::CycleColorMode => {
                self.cycle_color_mode(1);
                Outcome::Redraw
            }
            Action::CycleColorModeBack => {
                self.cycle_color_mode(-1);
                Outcome::Redraw
            }
            Action::Extract => {
                self.start_extract();
                Outcome::Redraw
            }
            Action::Descend => {
                self.descend()?;
                Outcome::Redraw
            }
            Action::OpenSearch => {
                self.begin_search_prompt();
                Outcome::Redraw
            }
            // Mode-local actions: routed via the mode's own `handle` before
            // we get here; one not consumed there is a no-op. The
            // session / mode-local split is declared per variant in
            // `Action::is_mode_local` (exhaustive match — a new variant
            // fails to compile there until categorised).
            other => {
                debug_assert!(
                    other.is_mode_local(),
                    "session action {other:?} has no apply arm"
                );
                Outcome::Unhandled
            }
        })
    }

    // ---------------------------------------------------------------------
    // Mode switching helpers
    // ---------------------------------------------------------------------

    fn jump_to(&mut self, target: ModeId) {
        let idx = self.frame().mode_index(target);
        if let Some(idx) = idx {
            self.set_active(idx);
        }
    }

    fn toggle_aux(&mut self, target: ModeId) {
        let f = self.frame();
        if f.modes[f.active].id() == target {
            let dest = f.last_primary.unwrap_or(0);
            if dest != f.active {
                self.set_active(dest);
            }
        } else if let Some(idx) = f.mode_index(target) {
            self.set_active(idx);
        }
    }

    /// Switch the active mode index. Updates `last_primary` on
    /// non-aux landings, captures the outgoing mode's position (when
    /// it tracks) and restores it on the incoming mode.
    fn set_active(&mut self, new_idx: usize) {
        let f = self.frame_mut();
        if new_idx == f.active {
            return;
        }
        capture_position(f);
        f.active = new_idx;
        if !f.modes[new_idx].is_aux() {
            f.last_primary = Some(new_idx);
        }
        restore_position(f);
    }

    /// Activate `mode_id` in the current frame and seek it to `pos` — the
    /// in-frame jump behind `Mode::select_jump`. Unlike `set_active`, the
    /// incoming position is the caller's `pos` (the jump target), not the
    /// outgoing mode's captured position. The target is typically Hex
    /// (aux), so `last_primary` is left pointing at the originating view so
    /// Back / the Hex toggle returns there.
    pub(super) fn jump_to_position(&mut self, mode_id: ModeId, pos: Position) {
        let Some(idx) = self.frame().mode_index(mode_id) else {
            self.flash = Some(format!("cannot jump: no {mode_id:?} view in this frame"));
            return;
        };
        let f = self.frame_mut();
        f.active = idx;
        if !f.modes[idx].is_aux() {
            f.last_primary = Some(idx);
        }
        f.position = pos;
        // Position the target (works for owns-scroll and caller-scrolled
        // modes alike), then signal the jump so the target can mark the
        // landed spot — Hex highlights the byte. `set_position` alone
        // (the restore path) never marks, so plain mode switches don't.
        restore_position(f);
        let source = f.source.clone();
        f.modes[idx].jump_position(pos, &source);
    }

    fn cycle_view(&mut self, direction: isize) {
        let f = self.frame();
        let n = f.modes.len();
        if n == 0 {
            return;
        }
        // Hex sits in the cycle only when there's no other data
        // view — i.e. binary files where Hex is the only thing to
        // look at. Info doesn't count as a data view: a stack of
        // [Hex, Info] would otherwise treat Info as the "primary"
        // and silently drop Hex out of Tab, leaving the user stuck
        // on Info.
        let has_data_primary = f
            .modes
            .iter()
            .any(|m| !m.is_aux() && !matches!(m.id(), ModeId::Info));
        let mut i = f.active;
        for _ in 0..n {
            i = if direction >= 0 {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            if i == self.frame().active {
                break;
            }
            let id = self.frame().modes[i].id();
            if matches!(id, ModeId::Help | ModeId::About) {
                continue;
            }
            if id == ModeId::Hex && has_data_primary {
                continue;
            }
            self.set_active(i);
            return;
        }
    }

    fn cycle_theme(&mut self, direction: isize) {
        self.current_theme = if direction >= 0 {
            self.current_theme.next()
        } else {
            self.current_theme.prev()
        };
        self.peek_theme = make_peek_theme(self.current_theme, self.peek_theme.style_mode);
        self.invalidate_all_views();
    }

    fn cycle_color_mode(&mut self, direction: isize) {
        self.peek_theme.style_mode = if direction >= 0 {
            self.peek_theme.style_mode.next()
        } else {
            self.peek_theme.style_mode.prev()
        };
        self.invalidate_all_views();
    }
}
