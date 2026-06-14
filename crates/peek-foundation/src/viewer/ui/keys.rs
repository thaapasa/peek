use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A single physical-key trigger: a `KeyCode` plus an optional Ctrl modifier.
/// SHIFT is treated as part of the keycode (e.g. `Char('N')` already implies shift).
#[derive(Copy, Clone)]
pub struct Binding {
    pub code: KeyCode,
    pub ctrl: bool,
    /// Show this key in help-screen labels. Always dispatched either way —
    /// muscle-memory aliases (e.g. `Ctrl+F` paging) work but don't crowd
    /// the help card; the manual lists them all.
    pub in_help: bool,
}

impl Binding {
    #[rustfmt::skip]
    pub const fn plain(code: KeyCode) -> Self { Self { code, ctrl: false, in_help: true } }
    #[rustfmt::skip]
    pub const fn ctrl(c: char) -> Self { Self { code: KeyCode::Char(c), ctrl: true, in_help: true } }
    #[rustfmt::skip]
    pub const fn hidden(self) -> Self { Self { in_help: false, ..self } }

    pub fn matches(self, key: KeyEvent) -> bool {
        if self.code != key.code {
            return false;
        }
        let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        self.ctrl == has_ctrl
    }

    /// Human-readable name of this key for help screens (e.g. `Up`,
    /// `Shift+Tab`, `Ctrl+c`). The single place key-display cosmetics
    /// live — `Action::label_keys` derives its labels from here.
    pub fn label(self) -> String {
        let key = match self.code {
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Up => "Up".to_string(),
            KeyCode::Down => "Down".to_string(),
            KeyCode::Left => "Left".to_string(),
            KeyCode::Right => "Right".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::PageUp => "PgUp".to_string(),
            KeyCode::PageDown => "PgDn".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::BackTab => "Shift+Tab".to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            other => format!("{other:?}"),
        };
        if self.ctrl {
            format!("Ctrl+{key}")
        } else {
            key
        }
    }
}

/// Every semantic key action peek's interactive viewers can take.
/// This enum is the single source of truth for physical keybindings.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    /// Exit the current viewer.
    Quit,
    /// Scroll up one line.
    ScrollUp,
    /// Scroll down one line.
    ScrollDown,
    /// Page scroll up.
    PageUp,
    /// Page scroll down.
    PageDown,
    /// Jump to the top of the current view.
    Top,
    /// Jump to the bottom of the current view.
    Bottom,
    /// Jump to the file-info view.
    SwitchInfo,
    /// Toggle the help overlay.
    ToggleHelp,
    /// Cycle through the file's view modes (e.g. SVG: rendered → XML
    /// source → info → rendered). Skips overlay-style aux modes (Help,
    /// About) and Hex (which has its own dedicated key) unless Hex is
    /// the only data view (binary files).
    CycleView,
    /// Cycle through the file's view modes in reverse (Shift+Tab).
    CycleViewBack,
    /// Cycle to the next theme.
    CycleTheme,
    /// Cycle to the previous theme (`T`).
    CycleThemeBack,
    /// Cycle the output color mode (truecolor → 256 → 16 → grayscale → plain).
    CycleColorMode,
    /// Cycle the color mode backward (`C`).
    CycleColorModeBack,
    /// Enter hex view from another viewer (or exit, in toggle mode).
    SwitchToHex,
    /// Toggle the about / status screen.
    SwitchToAbout,
    /// Cycle the image-render background (auto/black/white/checkerboard).
    CycleBackground,
    /// Cycle the image-render background backward (`B`).
    CycleBackgroundBack,
    /// Cycle the image rendering mode (full/block/geo/ascii/contour).
    CycleImageMode,
    /// Cycle the image rendering mode backward (`M`).
    CycleImageModeBack,
    /// Cycle the image fit mode (contain / fit-width / fit-height).
    CycleFitMode,
    /// Scroll the visible viewport one step left (FitHeight images).
    ScrollLeft,
    /// Scroll the visible viewport one step right (FitHeight images).
    ScrollRight,
    /// Toggle raw / pretty rendering inside ContentMode (structured
    /// JSON/YAML/TOML/XML only). No global fallback — modes that don't
    /// consume `r` ignore it.
    ToggleRawSource,
    /// Toggle the reconstructed-text overlay in paged image views
    /// (PDF): real words from the document's text layer are written
    /// over the rendered glyph cells at their page positions. Only
    /// offered when the renderer carries a text layer.
    ToggleTextOverlay,
    /// Toggle the line-number gutter in text views.
    ToggleLineNumbers,
    /// Toggle soft wrap in text views. When on, vertical scroll moves
    /// visual rows and Left/Right are inert; when off, lines truncate
    /// and Left/Right pan the viewport horizontally.
    ToggleSoftWrap,
    /// Play / pause an animated image.
    PlayPause,
    /// Step forward (`n`) through the active mode's item sequence:
    /// search match, animation frame, EPUB chapter / PDF page, font
    /// face, classfile method. One variant for every stepper — only
    /// one sequence is meaningful per mode, and the mode's `handle`
    /// arm plus its help entry ("Next / previous chapter") name what
    /// is stepped. Same pattern as `Extract` / `Descend`.
    Next,
    /// Step backward (`p` / `N`); counterpart of [`Action::Next`].
    Prev,
    /// Open the text-search prompt (searchable views).
    OpenSearch,
    /// Toggle the sticky parent-directory breadcrumb at the top of a
    /// scrolled listing TOC view.
    ToggleStickyParents,
    /// Extract the currently-selected sub-item to disk: a file in a
    /// listing TOC view, or the current frame in an animation view.
    /// Modes that don't have an extractable selection ignore it.
    Extract,
    /// Recursive peek: drill into the active mode's selection and
    /// push it onto the session stack as a fresh viewer state.
    Descend,
    /// Pop the current session off the stack. At stack depth 1 this
    /// exits the viewer; deeper, it returns to the parent session.
    Back,
    /// Go up one directory in a listing view: the on-disk directory
    /// browser opens the parent directory (seeding the cursor on the
    /// child we came from); a tree TOC hops the selection up a level.
    /// No-op in non-listing modes.
    ParentDir,
    /// Reflow streaming-table column widths from the currently-visible
    /// viewport. `RowsTableMode` only; no-op elsewhere.
    ReflowWidths,
    /// Toggle the header row on / off. `RowsTableMode` only; no-op
    /// elsewhere.
    ToggleHeader,
    /// Zoom the current graphic view in by one step. Anchored on the
    /// viewport centre so the pixel under the centre stays put.
    ZoomIn,
    /// Zoom the current graphic view out by one step. Same anchor as
    /// `ZoomIn`.
    ZoomOut,
    /// Reset zoom to 1× and pan to the origin.
    ZoomReset,
    /// Jump to a whole-number preset zoom: `1`..=`9` → 1×..=9×.
    ZoomPreset(u8),
}

impl Action {
    /// Physical keys that trigger this action — the single source of
    /// truth for the key map. `label_keys()` derives its help text from
    /// these, so a key and its label can't drift. Each binding list is
    /// wrapped in an inline `const { }` block so the `&[..]` array lives
    /// in static storage (user `const fn` calls aren't implicitly
    /// promoted). Edit this map to rebind.
    #[rustfmt::skip]
    pub fn bindings(self) -> &'static [Binding] {
        use Binding as B;
        use KeyCode::*;

        /// Binding list as a `'static` slice — const block forces static storage.
        macro_rules! binds {
            ($($b:expr),+ $(,)?) => { const { &[$($b),+] } };
        }

        match self {
            Action::Quit                => binds![B::plain(Char('q')), B::ctrl('c')],
            Action::ScrollUp            => binds![B::plain(Up), B::plain(Char('k'))],
            Action::ScrollDown          => binds![B::plain(Down), B::plain(Char('j'))],
            Action::PageUp              => binds![B::plain(PageUp), B::plain(Char('u')), B::ctrl('b').hidden(), B::ctrl('u').hidden()],
            Action::PageDown            => binds![B::plain(PageDown), B::plain(Char('d')), B::ctrl('f').hidden(), B::ctrl('d').hidden()],
            Action::Top                 => binds![B::plain(Home), B::plain(Char('g'))],
            Action::Bottom              => binds![B::plain(End), B::plain(Char('G'))],
            Action::SwitchInfo          => binds![B::plain(Char('i'))],
            Action::ToggleHelp          => binds![B::plain(Char('h')), B::plain(Char('?'))],
            Action::CycleView           => binds![B::plain(Tab)],
            Action::CycleViewBack       => binds![B::plain(BackTab)],
            Action::CycleTheme          => binds![B::plain(Char('t'))],
            Action::CycleThemeBack      => binds![B::plain(Char('T'))],
            Action::CycleColorMode      => binds![B::plain(Char('c'))],
            Action::CycleColorModeBack  => binds![B::plain(Char('C'))],
            Action::SwitchToHex         => binds![B::plain(Char('x'))],
            Action::SwitchToAbout       => binds![B::plain(Char('a'))],
            Action::CycleBackground     => binds![B::plain(Char('b'))],
            Action::CycleBackgroundBack => binds![B::plain(Char('B'))],
            Action::CycleImageMode      => binds![B::plain(Char('m'))],
            Action::CycleImageModeBack  => binds![B::plain(Char('M'))],
            Action::CycleFitMode        => binds![B::plain(Char('f'))],
            Action::ScrollLeft          => binds![B::plain(Left)],
            Action::ScrollRight         => binds![B::plain(Right)],
            Action::ToggleRawSource     => binds![B::plain(Char('r'))],
            Action::ToggleTextOverlay   => binds![B::plain(Char('o'))],
            Action::ToggleLineNumbers   => binds![B::plain(Char('l'))],
            Action::ToggleSoftWrap      => binds![B::plain(Char('w'))],
            Action::PlayPause           => binds![B::plain(Char(' '))],
            Action::Next                => binds![B::plain(Char('n'))],
            Action::Prev                => binds![B::plain(Char('p')), B::plain(Char('N'))],
            Action::OpenSearch          => binds![B::plain(Char('/'))],
            Action::ToggleStickyParents => binds![B::plain(Char('s'))],
            Action::Extract             => binds![B::plain(Char('e'))],
            Action::Descend             => binds![B::plain(Enter)],
            Action::Back                => binds![B::plain(Esc)],
            Action::ParentDir           => binds![B::plain(Backspace)],
            Action::ReflowWidths        => binds![B::plain(Char('R'))],
            Action::ToggleHeader        => binds![B::plain(Char('H'))],
            Action::ZoomIn              => binds![B::plain(Char('+')), B::plain(Char('='))],
            Action::ZoomOut             => binds![B::plain(Char('-'))],
            Action::ZoomReset           => binds![B::plain(Char('0'))],
            Action::ZoomPreset(n) => match n {
                1 => binds![B::plain(Char('1'))],
                2 => binds![B::plain(Char('2'))],
                3 => binds![B::plain(Char('3'))],
                4 => binds![B::plain(Char('4'))],
                5 => binds![B::plain(Char('5'))],
                6 => binds![B::plain(Char('6'))],
                7 => binds![B::plain(Char('7'))],
                8 => binds![B::plain(Char('8'))],
                9 => binds![B::plain(Char('9'))],
                _ => &[],
            },
        }
    }

    /// Human-readable label of the keys for help screens — the action's
    /// equivalent keys joined with ", " (e.g. "Up, k"). Derived from
    /// `bindings()`, skipping `hidden()` aliases; help entries join
    /// several actions' labels with " / ".
    pub fn label_keys(self) -> String {
        self.bindings()
            .iter()
            .filter(|b| b.in_help)
            .map(|b| b.label())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn matches(self, key: KeyEvent) -> bool {
        self.bindings().iter().any(|b| b.matches(key))
    }

    /// True for actions consumed by the active mode's `handle`; false
    /// for session-level actions `ViewerState::apply` implements
    /// (navigation, mode switching, theme cycling, extract / descend).
    /// Deliberately exhaustive: a new variant fails to compile here
    /// until its author declares which side handles it — the session
    /// dispatcher itself only matches the session group and routes the
    /// rest through one catch-all.
    pub fn is_mode_local(self) -> bool {
        match self {
            Action::Quit
            | Action::Back
            | Action::ScrollUp
            | Action::ScrollDown
            | Action::PageUp
            | Action::PageDown
            | Action::Top
            | Action::Bottom
            | Action::SwitchInfo
            | Action::ToggleHelp
            | Action::CycleView
            | Action::CycleViewBack
            | Action::CycleTheme
            | Action::CycleThemeBack
            | Action::CycleColorMode
            | Action::CycleColorModeBack
            | Action::SwitchToHex
            | Action::SwitchToAbout
            | Action::OpenSearch
            | Action::Extract
            | Action::Descend
            | Action::ParentDir => false,
            Action::ToggleRawSource
            | Action::ToggleTextOverlay
            | Action::PlayPause
            | Action::Next
            | Action::Prev
            | Action::CycleBackground
            | Action::CycleBackgroundBack
            | Action::CycleImageMode
            | Action::CycleImageModeBack
            | Action::CycleFitMode
            | Action::ScrollLeft
            | Action::ScrollRight
            | Action::ToggleLineNumbers
            | Action::ToggleSoftWrap
            | Action::ToggleStickyParents
            | Action::ReflowWidths
            | Action::ToggleHeader
            | Action::ZoomIn
            | Action::ZoomOut
            | Action::ZoomReset
            | Action::ZoomPreset(_) => true,
        }
    }

    /// True for every action that conceptually moves the viewport.
    /// Used by modes that need to consume scroll input when they have
    /// no content to scroll, so the action doesn't bubble up to a
    /// nonsensical global handler.
    pub fn is_scroll(self) -> bool {
        matches!(
            self,
            Action::ScrollUp
                | Action::ScrollDown
                | Action::PageUp
                | Action::PageDown
                | Action::Top
                | Action::Bottom
                | Action::ScrollLeft
                | Action::ScrollRight
        )
    }
}

/// One help-screen entry: a group of actions that share a description.
/// In the help screen the keys render joined with " / "; in dispatch any
/// action in the group matches. Most entries hold a single action — pair
/// only actions that read naturally together (e.g. next / previous).
pub type HelpEntry = (&'static [Action], &'static str);

/// Global actions that work in every mode (unless the mode shadows the
/// key via its own `extra_actions`). Used for both key dispatch and the
/// help screen.
pub const GLOBAL_ACTIONS: &[HelpEntry] = &[
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

/// Find the first action this viewer allows whose bindings match `key`.
/// Linear scan over a small `&'static` slice — sub-microsecond.
pub fn dispatch(key: KeyEvent, allowed: &[HelpEntry]) -> Option<Action> {
    allowed
        .iter()
        .flat_map(|(actions, _)| actions.iter())
        .find_map(|a| a.matches(key).then_some(*a))
}

/// Result of `ViewerState::apply` — what the event loop should do next.
pub enum Outcome {
    /// User wants to exit.
    Quit,
    /// State updated — caller should redraw the screen.
    Redraw,
    /// The action is not a global one; the active mode should handle it.
    Unhandled,
}
