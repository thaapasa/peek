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
    /// Cycle to the next theme.
    CycleTheme,
    /// Cycle the output color mode (truecolor → 256 → 16 → grayscale → plain).
    CycleColorMode,
    /// Enter hex view from another viewer (or exit, in toggle mode).
    SwitchToHex,
    /// Toggle the about / status screen.
    SwitchToAbout,
    /// Cycle the image-render background (auto/black/white/checkerboard).
    CycleBackground,
    /// Cycle the image rendering mode (full/block/geo/ascii).
    CycleImageMode,
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
}

impl Action {
    /// Physical keys that trigger this action. Edit this map to rebind.
    #[rustfmt::skip]
    pub fn bindings(self) -> &'static [Binding] {
        use KeyCode::*;

        // Each constant is the static binding list for one action. Defined
        // inside the function to keep the entire key map in one place.
        const QUIT:           &[Binding] = &[Binding::plain(Char('q')), Binding::plain(Esc), Binding::ctrl('c')];
        const SCROLL_UP:      &[Binding] = &[Binding::plain(Up),        Binding::plain(Char('k'))];
        const SCROLL_DOWN:    &[Binding] = &[Binding::plain(Down),      Binding::plain(Char('j'))];
        const PAGE_UP:        &[Binding] = &[Binding::plain(PageUp)];
        const PAGE_DOWN:      &[Binding] = &[Binding::plain(PageDown),  Binding::plain(Char(' '))];
        const TOP:            &[Binding] = &[Binding::plain(Home),      Binding::plain(Char('g'))];
        const BOTTOM:         &[Binding] = &[Binding::plain(End),       Binding::plain(Char('G'))];
        const SWITCH_INFO:    &[Binding] = &[Binding::plain(Char('i'))];
        const TOGGLE_HELP:    &[Binding] = &[Binding::plain(Char('h')), Binding::plain(Char('?'))];
        const CYCLE_VIEW:     &[Binding] = &[Binding::plain(Tab)];
        const CYCLE_THEME:    &[Binding] = &[Binding::plain(Char('t'))];
        const CYCLE_COLOR:    &[Binding] = &[Binding::plain(Char('c'))];
        const SWITCH_HEX:     &[Binding] = &[Binding::plain(Char('x'))];
        const SWITCH_ABOUT:   &[Binding] = &[Binding::plain(Char('a'))];
        const CYCLE_BG:       &[Binding] = &[Binding::plain(Char('b'))];
        const CYCLE_IMG_MODE: &[Binding] = &[Binding::plain(Char('m'))];
        const CYCLE_FIT:      &[Binding] = &[Binding::plain(Char('f'))];
        const SCROLL_LEFT:    &[Binding] = &[Binding::plain(Left)];
        const SCROLL_RIGHT:   &[Binding] = &[Binding::plain(Right)];
        const TOGGLE_RAW:     &[Binding] = &[Binding::plain(Char('r'))];
        const TOGGLE_LINENUM: &[Binding] = &[Binding::plain(Char('l'))];
        const TOGGLE_WRAP:    &[Binding] = &[Binding::plain(Char('w'))];
        const PLAY_PAUSE:     &[Binding] = &[Binding::plain(Char('p'))];
        const NEXT_FRAME:     &[Binding] = &[Binding::plain(Char('n'))];
        const PREV_FRAME:     &[Binding] = &[Binding::plain(Char('N'))];

        match self {
            Action::Quit              => QUIT,
            Action::ScrollUp          => SCROLL_UP,
            Action::ScrollDown        => SCROLL_DOWN,
            Action::PageUp            => PAGE_UP,
            Action::PageDown          => PAGE_DOWN,
            Action::Top               => TOP,
            Action::Bottom            => BOTTOM,
            Action::SwitchInfo        => SWITCH_INFO,
            Action::ToggleHelp        => TOGGLE_HELP,
            Action::CycleView         => CYCLE_VIEW,
            Action::CycleTheme        => CYCLE_THEME,
            Action::CycleColorMode    => CYCLE_COLOR,
            Action::SwitchToHex       => SWITCH_HEX,
            Action::SwitchToAbout     => SWITCH_ABOUT,
            Action::CycleBackground   => CYCLE_BG,
            Action::CycleImageMode    => CYCLE_IMG_MODE,
            Action::CycleFitMode      => CYCLE_FIT,
            Action::ScrollLeft        => SCROLL_LEFT,
            Action::ScrollRight       => SCROLL_RIGHT,
            Action::ToggleRawSource   => TOGGLE_RAW,
            Action::ToggleLineNumbers => TOGGLE_LINENUM,
            Action::ToggleSoftWrap    => TOGGLE_WRAP,
            Action::PlayPause         => PLAY_PAUSE,
            Action::NextFrame         => NEXT_FRAME,
            Action::PrevFrame         => PREV_FRAME,
        }
    }

    /// Human-readable label of the keys for help screens (e.g. "q / Esc").
    #[rustfmt::skip]
    pub fn label_keys(self) -> &'static str {
        match self {
            Action::Quit              => "q / Esc",
            Action::ScrollUp          => "Up / k",
            Action::ScrollDown        => "Down / j",
            Action::PageUp            => "PgUp",
            Action::PageDown          => "PgDn / Space",
            Action::Top               => "Home / g",
            Action::Bottom            => "End / G",
            Action::SwitchInfo        => "i",
            Action::ToggleHelp        => "h / ?",
            Action::CycleView         => "Tab",
            Action::CycleTheme        => "t",
            Action::CycleColorMode    => "c",
            Action::SwitchToHex       => "x",
            Action::SwitchToAbout     => "a",
            Action::CycleBackground   => "b",
            Action::CycleImageMode    => "m",
            Action::CycleFitMode      => "f",
            Action::ScrollLeft        => "Left",
            Action::ScrollRight       => "Right",
            Action::ToggleRawSource   => "r",
            Action::ToggleLineNumbers => "l",
            Action::ToggleSoftWrap    => "w",
            Action::PlayPause         => "p",
            Action::NextFrame         => "n",
            Action::PrevFrame         => "N",
        }
    }

    pub fn matches(self, key: KeyEvent) -> bool {
        self.bindings().iter().any(|b| b.matches(key))
    }
}

/// Find the first action this viewer allows whose bindings match `key`.
/// Linear scan over a small `&'static` slice — sub-microsecond.
pub(crate) fn dispatch(key: KeyEvent, allowed: &[(Action, &'static str)]) -> Option<Action> {
    allowed
        .iter()
        .find_map(|(a, _)| a.matches(key).then_some(*a))
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
