use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::KeyEvent;

use crate::info::{FileInfo, RenderOptions};
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::theme::{PeekTheme, PeekThemeName, StyleMode};
use crate::viewer::modes::{Handled, Mode, ModeId, Position, RenderCtx};

use super::keys::{self, Action, HelpEntry, Outcome};
use super::prompt::{Prompt, PromptOutcome};
use super::screen::ScreenBuffer;
use super::{content_rows, make_peek_theme, terminal_cols};
use crate::extract::Extracted;

/// One mode's most recent windowed render. The `lines` field is the
/// exact slice that should be drawn at the top of the viewport; the
/// `scroll_at` and `rows_at` fields are the inputs the mode was given,
/// used as the cache key. `total` is the full-source line count so
/// scroll math (max_scroll, Bottom jump) doesn't need to re-render.
pub(crate) struct RenderedView {
    lines: Vec<String>,
    scroll_at: usize,
    rows_at: usize,
    total: usize,
}

/// Global actions that work in every mode (unless the mode shadows the
/// key via its own `extra_actions`). Used for both key dispatch and the
/// help screen.
pub(crate) const GLOBAL_ACTIONS: &[HelpEntry] = &[
    (&[Action::Quit], "Quit"),
    (&[Action::Back], "Back / close current peek"),
    (&[Action::ScrollUp, Action::ScrollDown], "Scroll up / down"),
    (&[Action::PageUp, Action::PageDown], "Page up / down"),
    (&[Action::Top, Action::Bottom], "Jump to top / bottom"),
    (
        &[Action::CycleView, Action::CycleViewBack],
        "Cycle file's view modes (fwd / back)",
    ),
    (&[Action::SwitchInfo], "File info"),
    (&[Action::ToggleHelp], "Toggle help"),
    (&[Action::SwitchToHex], "Hex dump mode"),
    (&[Action::SwitchToAbout], "About / status screen"),
    (
        &[Action::CycleTheme, Action::CycleThemeBack],
        "Next / previous theme",
    ),
    (
        &[Action::CycleColorMode, Action::CycleColorModeBack],
        "Next / previous color mode",
    ),
    (&[Action::Extract], "Extract selected entry / current frame"),
    (&[Action::Descend], "Descend into selected entry / frame"),
];

/// Hard cap on session-stack depth. Real listings rarely nest beyond
/// 3–4 levels; the cap exists so a hostile container that recursively
/// resolves to itself can't grow the stack without bound.
const MAX_STACK_DEPTH: usize = 16;

/// Builds the mode stack for a freshly-pushed session. Captured at
/// `ViewerState` construction so `descend` doesn't have to know about
/// `Registry` / `Args`.
pub(crate) type ModeBuilder = Box<dyn Fn(&InputSource, &Detected) -> Result<Vec<Box<dyn Mode>>>>;

/// What the modal [`Prompt`] is collecting input for — the action to run
/// when the user confirms. Lets one prompt slot serve both the
/// extract-save flow and text search.
enum PromptKind {
    /// Save the extracted item to the typed path.
    Extract(Extracted),
    /// Hand the typed query to the active mode's `set_search`.
    Search,
}

/// One peek session — one `(source, detected, modes)` triple plus its
/// per-mode scroll / view cache / position state. The recursive-peek
/// stack is a `Vec<SessionFrame>`; the active session is always the
/// last entry. Cross-session state (theme, prompt overlay, screen
/// buffer) lives directly on `ViewerState`.
pub(crate) struct SessionFrame {
    pub source: InputSource,
    pub detected: Detected,
    pub file_info: FileInfo,
    pub modes: Vec<Box<dyn Mode>>,
    pub active: usize,
    /// Most recent primary (non-aux) mode. Aux toggles return here.
    /// `None` when no primary modes exist (binary files where Hex is
    /// the only data view).
    pub last_primary: Option<usize>,
    pub scroll: Vec<usize>,
    pub views: Vec<Option<RenderedView>>,
    /// Last known logical position; restored when modes that track
    /// position become active again.
    pub position: Position,
    /// One-shot retry guard: when a render fails on this frame, we try
    /// re-detecting the source with `detect_ignore_name` and rebuild the
    /// frame. Set after that retry runs (success or not) so a second
    /// render failure on the rebuilt frame propagates rather than
    /// looping.
    pub retry_attempted: bool,
}

impl SessionFrame {
    fn new(
        source: InputSource,
        detected: Detected,
        file_info: FileInfo,
        modes: Vec<Box<dyn Mode>>,
    ) -> Self {
        assert!(!modes.is_empty(), "SessionFrame needs at least one mode");
        let n = modes.len();
        let last_primary = if modes[0].is_aux() { None } else { Some(0) };
        Self {
            source,
            detected,
            file_info,
            modes,
            active: 0,
            last_primary,
            scroll: vec![0; n],
            views: (0..n).map(|_| None).collect(),
            position: Position::Unknown,
            retry_attempted: false,
        }
    }

    fn mode_index(&self, id: ModeId) -> Option<usize> {
        self.modes.iter().position(|m| m.id() == id)
    }
}

pub(crate) struct ViewerState {
    /// Recursive-peek stack. Always non-empty while the viewer runs;
    /// the last `Back` on a single-frame stack returns `Outcome::Quit`.
    frames: Vec<SessionFrame>,

    /// Builds the mode stack for a freshly-pushed session. See
    /// [`ModeBuilder`].
    mode_builder: ModeBuilder,

    pub current_theme: PeekThemeName,
    pub peek_theme: PeekTheme,

    /// Frame buffer: caches the previous draw, skips writes for
    /// unchanged rows. Invalidated on resize and on stack push/pop.
    screen: ScreenBuffer,
    render_opts: RenderOptions,

    /// Modal prompt overlay. While `Some`, raw key events go to the
    /// prompt and the status line shows its render. The paired
    /// [`PromptKind`] is the work to run on confirm — keeps the Prompt
    /// widget oblivious to its purpose.
    prompt: Option<(Prompt, PromptKind)>,

    /// One-shot status flash (e.g. "wrote /tmp/foo"). Cleared after
    /// one redraw.
    flash: Option<String>,

    /// Mirror of the CLI `--no-tempfile` flag. Threaded into every
    /// `ExtractOptions` the interactive viewer builds so user choice
    /// persists across descend / extract presses.
    no_tempfile: bool,
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
        let file_info = crate::info::gather(&source, &detected)?;
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

    fn frame_mut(&mut self) -> &mut SessionFrame {
        self.frames.last_mut().expect("non-empty stack")
    }

    #[allow(dead_code)]
    pub(crate) fn stack_depth(&self) -> usize {
        self.frames.len()
    }

    /// Display names of every frame on the stack (root first), used
    /// by the status line to render the breadcrumb segment.
    pub(crate) fn breadcrumb(&self) -> Vec<String> {
        self.frames
            .iter()
            .map(|f| f.source.name().to_string())
            .collect()
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
        keys::dispatch(key, GLOBAL_ACTIONS).or_else(|| keys::dispatch(key, extras))
    }

    pub(crate) fn prompt_active(&self) -> bool {
        self.prompt.is_some()
    }

    pub(crate) fn active_prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref().map(|(p, _)| p)
    }

    pub(crate) fn take_flash(&mut self) -> Option<String> {
        self.flash.take()
    }

    /// Open the save-to prompt; Enter writes `extracted` to the typed
    /// path, Esc drops it without writing.
    pub(crate) fn begin_extract_prompt(&mut self, extracted: Extracted) {
        let prefill = extracted.suggested_name.clone();
        self.prompt = Some((
            Prompt::new("Save to", prefill),
            PromptKind::Extract(extracted),
        ));
    }

    /// Open the text-search prompt; Enter hands the query to the active
    /// mode's `set_search`, Esc closes without changing the search.
    fn begin_search_prompt(&mut self) {
        self.prompt = Some((Prompt::new("Search", ""), PromptKind::Search));
    }

    pub(crate) fn handle_prompt_key(&mut self, key: KeyEvent) -> Result<bool> {
        let Some((prompt, _)) = self.prompt.as_mut() else {
            return Ok(false);
        };
        let outcome = prompt.handle_key(key);
        match outcome {
            PromptOutcome::Continue => Ok(true),
            PromptOutcome::Cancelled => {
                let (_, kind) = self.prompt.take().expect("prompt present");
                if matches!(kind, PromptKind::Extract(_)) {
                    self.flash = Some("extract cancelled".to_string());
                }
                Ok(true)
            }
            PromptOutcome::Confirmed(value) => {
                let (_, kind) = self.prompt.take().expect("prompt present");
                match kind {
                    PromptKind::Extract(extracted) => {
                        let dest = if value.is_empty() {
                            crate::extract::write::Output::resolve(None, &extracted.suggested_name)
                        } else if value == "-" {
                            crate::extract::write::Output::Stdout
                        } else {
                            crate::extract::write::Output::Path(value.into())
                        };
                        match crate::extract::write::write_extracted(&extracted, dest) {
                            Ok(path) => {
                                self.flash = Some(format!("wrote {}", path.display()));
                            }
                            Err(e) => {
                                self.flash = Some(format!("extract failed: {e}"));
                            }
                        }
                    }
                    PromptKind::Search => {
                        let query = (!value.is_empty()).then_some(value.as_str());
                        {
                            let f = self.frame_mut();
                            let active = f.active;
                            let target = f.modes[active].set_search(query);
                            // Modes that own their scroll position
                            // themselves; others get scrolled to the
                            // first match here.
                            if let Some(line) = target
                                && !f.modes[active].owns_scroll()
                            {
                                f.scroll[active] = line;
                            }
                        }
                        self.invalidate_active();
                    }
                }
                Ok(true)
            }
        }
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
            // we get here. Listed explicitly so adding a new Action variant
            // forces a non-exhaustive-match compile error in this function
            // and a deliberate decision about which side handles it.
            Action::ToggleRawSource
            | Action::PlayPause
            | Action::NextFrame
            | Action::PrevFrame
            | Action::NextChapter
            | Action::PrevChapter
            | Action::NextMatch
            | Action::PrevMatch
            | Action::CycleBackground
            | Action::CycleImageMode
            | Action::CycleFitMode
            | Action::ScrollLeft
            | Action::ScrollRight
            | Action::ToggleLineNumbers
            | Action::ToggleSoftWrap
            | Action::CycleBackgroundBack
            | Action::CycleImageModeBack
            | Action::ToggleStickyParents
            | Action::ReflowWidths
            | Action::ToggleHeader => Outcome::Unhandled,
        })
    }

    fn extract_target_key(&mut self) -> Option<String> {
        let f = self.frame();
        let target = f.modes[f.active].extract_target()?;
        Some(match target {
            crate::viewer::modes::ExtractTarget::EntryPath(p) => p,
            crate::viewer::modes::ExtractTarget::FrameIndex(n) => n.to_string(),
        })
    }

    /// Run extract against the active mode's selection, then open the
    /// save-to prompt. Failures flash on the status line.
    fn start_extract(&mut self) {
        let Some(key) = self.extract_target_key() else {
            self.flash = Some("nothing selected to extract".to_string());
            return;
        };
        let opts = crate::extract::ExtractOptions {
            no_tempfile: self.no_tempfile,
            ..Default::default()
        };
        let f = self.frame();
        match crate::extract::extract(&f.source, &f.detected, &key, &opts) {
            Ok(extracted) => self.begin_extract_prompt(extracted),
            Err(e) => self.flash = Some(format!("extract failed: {e}")),
        }
    }

    /// Recursive peek: extract the active mode's selection and push it
    /// as a new session on the stack. Failures (no selection,
    /// unsupported, broken entry, stack full) flash and leave the
    /// current frame active.
    fn descend(&mut self) -> Result<()> {
        if self.frames.len() >= MAX_STACK_DEPTH {
            self.flash = Some(format!("peek stack at max depth ({MAX_STACK_DEPTH})"));
            return Ok(());
        }
        let Some(key) = self.extract_target_key() else {
            self.flash = Some("nothing to descend into".to_string());
            return Ok(());
        };
        let opts = crate::extract::ExtractOptions {
            no_tempfile: self.no_tempfile,
            ..Default::default()
        };
        let extracted = {
            let f = self.frame();
            match crate::extract::extract(&f.source, &f.detected, &key, &opts) {
                Ok(e) => e,
                Err(e) => {
                    self.flash = Some(format!("descend failed: {e}"));
                    return Ok(());
                }
            }
        };
        self.push_extracted(extracted)
    }

    fn push_extracted(&mut self, extracted: Extracted) -> Result<()> {
        let source = extracted.source;
        let detected = match crate::input::detect::detect(&source) {
            Ok(d) => d,
            Err(e) => {
                self.flash = Some(format!("descend failed: {e}"));
                return Ok(());
            }
        };
        // Apply transparent decompression so descending into an
        // extracted `.gz` / `.bz2` / `.xz` / `.zst` / `.lz4` lands
        // straight on the inner content.
        let (source, detected) = crate::input::compression::resolve_transparent(source, detected);
        let modes = match (self.mode_builder)(&source, &detected) {
            Ok(m) => m,
            Err(e) => {
                self.flash = Some(format!("descend failed: {e}"));
                return Ok(());
            }
        };
        let file_info = crate::info::gather(&source, &detected)?;
        let frame = SessionFrame::new(source, detected, file_info, modes);
        // Dir → Dir descent re-targets the current frame instead of
        // pushing, so navigating between sibling subdirectories doesn't
        // accumulate a stack the user has to back out of. Esc on the
        // resulting frame still exits peek (depth-1 Back semantics).
        let collapse = matches!(
            frame.detected.file_type,
            crate::input::detect::FileType::Directory
        ) && matches!(
            self.frame().detected.file_type,
            crate::input::detect::FileType::Directory
        );
        if collapse {
            *self.frames.last_mut().expect("non-empty stack") = frame;
        } else {
            self.frames.push(frame);
        }
        self.screen.invalidate();
        Ok(())
    }

    fn pop_frame(&mut self) {
        if self.frames.len() <= 1 {
            return;
        }
        self.frames.pop();
        self.screen.invalidate();
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

    /// Themes / color modes are global; staling every frame's view
    /// cache stops a pop-into-old-frame from showing stale colours.
    fn invalidate_all_views(&mut self) {
        for frame in &mut self.frames {
            for slot in &mut frame.views {
                *slot = None;
            }
        }
    }

    // ---------------------------------------------------------------------
    // Resize
    // ---------------------------------------------------------------------

    pub(crate) fn handle_resize(&mut self) {
        let cols = terminal_cols();
        let rows = content_rows();
        for frame in &mut self.frames {
            for (i, m) in frame.modes.iter_mut().enumerate() {
                m.on_resize(cols, rows);
                if m.rerender_on_resize() {
                    frame.views[i] = None;
                }
            }
        }
        self.screen.invalidate();
    }

    // ---------------------------------------------------------------------
    // Rendering
    // ---------------------------------------------------------------------

    pub(crate) fn ensure_active_rendered(&mut self) -> Result<()> {
        let (active, scroll) = {
            let f = self.frame();
            (f.active, f.scroll[f.active])
        };
        let rows = content_rows();
        let cache_hit = self.frame().views[active]
            .as_ref()
            .is_some_and(|v| v.scroll_at == scroll && v.rows_at == rows);
        if !cache_hit {
            match self.render_active() {
                Ok(view) => {
                    self.frame_mut().views[active] = Some(view);
                }
                Err(e) => {
                    // Frame may have been built from a name-biased detect
                    // (file extension lied about the content). Try
                    // magic-byte-only re-detection once; if it yields a
                    // different file type, rebuild the frame and retry
                    // the render. Applies uniformly to root and nested
                    // descended frames.
                    if !self.frame().retry_attempted && self.retry_frame_detection()? {
                        let active = self.frame().active;
                        let view = self.render_active()?;
                        self.frame_mut().views[active] = Some(view);
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    /// Re-detect the active frame's source without using its path /
    /// entry name, rebuild modes + file_info if the classification
    /// changed, and reset cached views. Sets `retry_attempted` whether
    /// or not the classification changed so the caller doesn't loop.
    /// Returns `Ok(true)` when the frame was rebuilt and is worth
    /// re-rendering, `Ok(false)` when re-detection didn't change the
    /// type.
    fn retry_frame_detection(&mut self) -> Result<bool> {
        let retried = {
            let frame = self.frame();
            match crate::input::detect::detect_ignore_name(&frame.source) {
                Ok(d) if d.file_type != frame.detected.file_type => d,
                _ => {
                    self.frame_mut().retry_attempted = true;
                    return Ok(false);
                }
            }
        };
        // Re-detect-on-magic may surface a bare codec the name hid —
        // resolve transparently so the rebuilt frame renders the
        // decompressed inner content.
        let (source_clone, retried) =
            crate::input::compression::resolve_transparent(self.frame().source.clone(), retried);
        let modes = (self.mode_builder)(&source_clone, &retried)?;
        let file_info = crate::info::gather(&source_clone, &retried)?;
        let n = modes.len();
        let frame = self.frame_mut();
        frame.source = source_clone;
        frame.detected = retried;
        frame.file_info = file_info;
        frame.modes = modes;
        frame.active = 0;
        frame.last_primary = if frame.modes[0].is_aux() {
            None
        } else {
            Some(0)
        };
        frame.scroll = vec![0; n];
        frame.views = (0..n).map(|_| None).collect();
        frame.position = Position::Unknown;
        frame.retry_attempted = true;
        // Drop the ScreenBuffer's row-diff cache so the next draw
        // repaints every row — the rebuilt frame's mode set, status
        // line, and content can differ from whatever the parent frame
        // (or earlier render attempt) left on screen.
        self.screen.invalidate();
        Ok(true)
    }

    pub(crate) fn invalidate_active(&mut self) {
        let f = self.frame_mut();
        let active = f.active;
        f.views[active] = None;
    }

    fn render_active(&mut self) -> Result<RenderedView> {
        let theme_name = self.current_theme;
        let render_opts = self.render_opts;
        let term_cols_v = terminal_cols();
        let rows = content_rows();
        // Borrow theme separately from the frame's mutable borrow —
        // `peek_theme` lives on `self`, not on the frame, so the two
        // disjoint accesses don't alias.
        let peek_theme = self.peek_theme.clone();
        let f = self.frame_mut();
        let active = f.active;
        let scroll = f.scroll[active];
        let window = {
            let ctx = RenderCtx {
                file_info: &f.file_info,
                theme_name,
                peek_theme: &peek_theme,
                render_opts,
                term_cols: term_cols_v,
                term_rows: rows,
            };
            f.modes[active].render_window(&ctx, scroll, rows)?
        };
        let new_warnings = f.modes[active].take_warnings();
        if !new_warnings.is_empty() {
            f.file_info.warnings.extend(new_warnings);
            if let Some(idx) = f.mode_index(ModeId::Info) {
                f.views[idx] = None;
            }
        }
        Ok(RenderedView {
            lines: window.lines,
            scroll_at: scroll,
            rows_at: rows,
            total: window.total,
        })
    }

    // ---------------------------------------------------------------------
    // Line scrolling (used when active mode does NOT own scroll)
    // ---------------------------------------------------------------------

    fn max_scroll(&self) -> usize {
        let f = self.frame();
        let total = f.views[f.active].as_ref().map_or(0, |v| v.total);
        total.saturating_sub(content_rows())
    }

    fn prepare_total(&mut self) -> Result<()> {
        let (active, total_lines, has_view) = {
            let f = self.frame();
            (
                f.active,
                f.modes[f.active].total_lines(),
                f.views[f.active].is_some(),
            )
        };
        if let Some(n) = total_lines {
            let needs_seed = self.frame().views[active]
                .as_ref()
                .is_none_or(|v| v.total != n);
            if needs_seed {
                self.frame_mut().views[active] = Some(RenderedView {
                    lines: Vec::new(),
                    scroll_at: usize::MAX,
                    rows_at: content_rows(),
                    total: n,
                });
            }
            return Ok(());
        }
        if !has_view {
            self.ensure_active_rendered()?;
        }
        Ok(())
    }

    fn scroll_by(&mut self, delta: isize) -> Result<()> {
        if self.frame().modes[self.frame().active].owns_scroll() {
            return Ok(());
        }
        self.prepare_total()?;
        let max = self.max_scroll();
        let f = self.frame_mut();
        let active = f.active;
        let s = &mut f.scroll[active];
        *s = if delta < 0 {
            s.saturating_sub((-delta) as usize)
        } else {
            (*s + delta as usize).min(max)
        };
        Ok(())
    }

    fn page(&mut self, direction: isize) -> Result<()> {
        if self.frame().modes[self.frame().active].owns_scroll() {
            return Ok(());
        }
        self.prepare_total()?;
        let step = content_rows().saturating_sub(1);
        let max = self.max_scroll();
        let f = self.frame_mut();
        let active = f.active;
        let s = &mut f.scroll[active];
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

    pub(crate) fn draw(&mut self, stdout: &mut io::Stdout, status: &str) -> Result<()> {
        let reset_bytes = self.peek_theme.style_mode.reset_bytes();
        // Clone the visible slice into an owned Vec so the immutable
        // frame borrow doesn't conflict with the &mut self.screen
        // call below. Per-frame this is one shallow copy of the
        // viewport's String pointers — no glyph data duplicated.
        let lines: Vec<String> = {
            let f = self.frame();
            f.views[f.active]
                .as_ref()
                .map(|v| v.lines.clone())
                .unwrap_or_default()
        };
        self.screen.draw(stdout, &lines, status, reset_bytes)
    }
}

fn capture_position(f: &mut SessionFrame) {
    let mode = &f.modes[f.active];
    if !mode.tracks_position() {
        return;
    }
    let pos = if mode.owns_scroll() {
        mode.position()
    } else {
        Position::Line(f.scroll[f.active])
    };
    if !matches!(pos, Position::Unknown) {
        f.position = pos;
    }
}

fn restore_position(f: &mut SessionFrame) {
    let pos = f.position;
    let active = f.active;
    let source = f.source.clone();
    let mode = &mut f.modes[active];
    if !mode.tracks_position() {
        return;
    }
    if mode.owns_scroll() {
        mode.set_position(pos, &source);
        return;
    }
    let line = match pos {
        Position::Line(l) => Some(l),
        Position::Byte(b) => source.byte_to_line(b),
        Position::Unknown => None,
    };
    if let Some(l) = line {
        f.scroll[active] = l;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Args;
    use crate::viewer::Registry;
    use clap::Parser;
    use crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};
    use std::rc::Rc;

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
        let registry = Rc::new(Registry::new(&args).unwrap());
        let modes = registry.compose_modes(&source, &detected, &args).unwrap();
        let registry_for_builder = registry.clone();
        let args_for_builder = args.clone();
        let mode_builder: ModeBuilder =
            Box::new(move |s, d| registry_for_builder.compose_modes(s, d, &args_for_builder));
        ViewerState::new(
            source,
            detected,
            args.theme,
            args.color,
            RenderOptions::default(),
            modes,
            mode_builder,
            args.no_tempfile,
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
        let detected = crate::input::detect::detect(&source).unwrap();
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

    #[test]
    fn scrolldown_on_info_after_tab_advances_scroll() {
        let source = fixture_source("test-images/calendar.svg");
        let detected = crate::input::detect::detect(&source).unwrap();
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
        let static_det = crate::input::detect::detect(&static_src).unwrap();
        let mut static_state = build_state(
            &["peek", "test-images/calendar.svg"],
            static_src,
            static_det,
        );

        let anim_src = fixture_source("test-images/loader-dots.svg");
        let anim_det = crate::input::detect::detect(&anim_src).unwrap();
        let mut anim_state =
            build_state(&["peek", "test-images/loader-dots.svg"], anim_src, anim_det);

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
        let _term = crate::viewer::ui::test_term_override::pin(80, 21);

        let source = fixture_source("test-images/walking-outside.svg");
        let detected = crate::input::detect::detect(&source).unwrap();
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
        let detected = crate::input::detect::detect(&source).unwrap();
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

    /// Directory descent into a subdirectory must collapse the new
    /// frame onto the current one — no stack of dirs to back out of.
    /// Descending into a regular file *does* push (so Back returns to
    /// the listing), and Esc on a depth-1 directory frame quits.
    #[test]
    fn directory_subdir_descent_replaces_frame() {
        // src/ has subdirectories. Row 0 is the synthetic `..`; skip
        // past it so we exercise descent into a real child dir.
        let source = fixture_source("src");
        let detected = crate::input::detect::detect(&source).unwrap();
        assert!(matches!(
            detected.file_type,
            crate::input::detect::FileType::Directory
        ));
        let mut state = build_state(&["peek", "src"], source, detected);
        assert_eq!(state.stack_depth(), 1);
        state.try_active_scroll(Action::ScrollDown);
        state.apply(Action::Descend).unwrap();
        assert_eq!(state.stack_depth(), 1, "dir → dir descent collapses stack");
        assert!(matches!(
            state.frame().detected.file_type,
            crate::input::detect::FileType::Directory
        ));
    }

    /// Descending from a directory into a regular file pushes a new
    /// frame so Back returns to the listing.
    #[test]
    fn directory_file_descent_pushes_frame() {
        // test-data/ contains only files. Row 0 is the synthetic
        // `..`; advance past it to land on a real file row.
        let source = fixture_source("test-data");
        let detected = crate::input::detect::detect(&source).unwrap();
        let mut state = build_state(&["peek", "test-data"], source, detected);
        assert_eq!(state.stack_depth(), 1);
        state.try_active_scroll(Action::ScrollDown);
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
        let detected = crate::input::detect::detect(&source).unwrap();
        let mut state = build_state(&["peek", "src"], source, detected);
        // `..` is row 0 by construction.
        state.apply(Action::Descend).unwrap();
        assert_eq!(state.stack_depth(), 1, ".. descent stays at depth 1");
        let new_path = state
            .frame()
            .source
            .path()
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
        let detected = crate::input::detect::detect(&source).unwrap();
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
        let detected = crate::input::detect::detect(&source).unwrap();
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
}
