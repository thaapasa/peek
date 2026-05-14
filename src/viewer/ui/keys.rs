use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A single physical-key trigger: a `KeyCode` plus an optional Ctrl modifier.
/// SHIFT is treated as part of the keycode (e.g. `Char('N')` already implies shift).
#[derive(Copy, Clone)]
pub(crate) struct Binding {
    pub code: KeyCode,
    pub ctrl: bool,
}

impl Binding {
    #[rustfmt::skip]
    pub const fn plain(code: KeyCode) -> Self { Self { code, ctrl: false } }
    #[rustfmt::skip]
    pub const fn ctrl(c: char) -> Self { Self { code: KeyCode::Char(c), ctrl: true } }

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
pub(crate) enum Action {
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
    /// Cycle the image rendering mode (full/block/geo/ascii).
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
    /// Toggle the line-number gutter in text views.
    ToggleLineNumbers,
    /// Toggle soft wrap in text views. When on, vertical scroll moves
    /// visual rows and Left/Right are inert; when off, lines truncate
    /// and Left/Right pan the viewport horizontally.
    ToggleSoftWrap,
    /// Play / pause an animated image.
    PlayPause,
    /// Advance to the next animation frame.
    NextFrame,
    /// Step back to the previous animation frame.
    PrevFrame,
    /// Advance to the next chapter (EPUB read mode).
    NextChapter,
    /// Step back to the previous chapter (EPUB read mode).
    PrevChapter,
    /// Open the text-search prompt (ContentMode).
    OpenSearch,
    /// Jump to the next search match (ContentMode).
    NextMatch,
    /// Jump to the previous search match (ContentMode).
    PrevMatch,
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
            Action::PageUp              => binds![B::plain(PageUp)],
            Action::PageDown            => binds![B::plain(PageDown)],
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
            Action::ToggleLineNumbers   => binds![B::plain(Char('l'))],
            Action::ToggleSoftWrap      => binds![B::plain(Char('w'))],
            Action::PlayPause           => binds![B::plain(Char(' '))],
            Action::NextFrame           => binds![B::plain(Char('n'))], // n — also NextChapter / NextMatch (different mode)
            Action::PrevFrame           => binds![B::plain(Char('p'))], // p — also PrevChapter / PrevMatch (different mode)
            Action::NextChapter         => binds![B::plain(Char('n'))],
            Action::PrevChapter         => binds![B::plain(Char('p'))],
            Action::OpenSearch          => binds![B::plain(Char('/'))],
            Action::NextMatch           => binds![B::plain(Char('n'))],
            Action::PrevMatch           => binds![B::plain(Char('p'))],
            Action::ToggleStickyParents => binds![B::plain(Char('s'))],
            Action::Extract             => binds![B::plain(Char('e'))],
            Action::Descend             => binds![B::plain(Enter)],
            Action::Back                => binds![B::plain(Esc)],
        }
    }

    /// Human-readable label of the keys for help screens — the action's
    /// equivalent keys joined with ", " (e.g. "Up, k"). Derived from
    /// `bindings()`; help entries join several actions' labels with " / ".
    pub fn label_keys(self) -> String {
        self.bindings()
            .iter()
            .map(|b| b.label())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn matches(self, key: KeyEvent) -> bool {
        self.bindings().iter().any(|b| b.matches(key))
    }
}

/// One help-screen entry: a group of actions that share a description.
/// In the help screen the keys render joined with " / "; in dispatch any
/// action in the group matches. Most entries hold a single action — pair
/// only actions that read naturally together (e.g. next / previous).
pub(crate) type HelpEntry = (&'static [Action], &'static str);

/// Find the first action this viewer allows whose bindings match `key`.
/// Linear scan over a small `&'static` slice — sub-microsecond.
pub(crate) fn dispatch(key: KeyEvent, allowed: &[HelpEntry]) -> Option<Action> {
    allowed
        .iter()
        .flat_map(|(actions, _)| actions.iter())
        .find_map(|a| a.matches(key).then_some(*a))
}

/// Result of `ViewerState::apply` — what the event loop should do next.
pub(crate) enum Outcome {
    /// User wants to exit.
    Quit,
    /// State updated — caller should redraw the screen.
    Redraw,
    /// The action is not a global one; the active mode should handle it.
    Unhandled,
}
