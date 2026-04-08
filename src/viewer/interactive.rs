use std::cell::Cell;
use std::io;
use std::path::Path;
use std::rc::Rc;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};

use crate::detect::FileType;
use crate::theme::PeekThemeName;
use crate::viewer::image::Background;
use crate::viewer::ui::{
    KeyAction, ViewerState, render_themed_status_line, with_alternate_screen,
};

// ---------------------------------------------------------------------------
// Help keys for the generic interactive viewer
// ---------------------------------------------------------------------------

const HELP_KEYS: &[(&str, &str)] = &[
    ("q / Esc", "Quit"),
    ("Up / k", "Scroll up"),
    ("Down / j", "Scroll down"),
    ("PgUp / PgDn", "Page scroll"),
    ("Space", "Page down"),
    ("Home / End", "Top / bottom"),
    ("Tab", "Toggle content / file info"),
    ("i", "File info"),
    ("h / ?", "Toggle help"),
    ("t", "Next theme"),
    ("r", "Toggle raw / pretty"),
    ("b", "Cycle background (images)"),
];

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Generic interactive viewer with alternate screen, scrolling, event loop,
/// Tab/i view switching, theme cycling, and help screen.
///
/// `render_content` produces the content lines for a given theme.
/// If `rerender_on_resize` is true, content is re-rendered on terminal resize
/// (needed for images whose output depends on terminal dimensions).
pub fn view_interactive(
    path: &Path,
    file_type: &FileType,
    theme_name: PeekThemeName,
    rerender_on_resize: bool,
    pretty: bool,
    render_content: impl Fn(PeekThemeName, bool) -> Result<Vec<String>>,
) -> Result<()> {
    view_interactive_with_bg(path, file_type, theme_name, rerender_on_resize, pretty, None, render_content)
}

/// Interactive viewer with optional background cycling support.
/// When `background` is `Some`, the `b` key cycles the background mode.
pub fn view_interactive_with_bg(
    path: &Path,
    file_type: &FileType,
    theme_name: PeekThemeName,
    rerender_on_resize: bool,
    pretty: bool,
    background: Option<Rc<Cell<Background>>>,
    render_content: impl Fn(PeekThemeName, bool) -> Result<Vec<String>>,
) -> Result<()> {
    with_alternate_screen(|stdout| {
        run_event_loop(
            stdout, path, file_type, theme_name,
            rerender_on_resize, pretty, background, &render_content,
        )
    })
}

// ---------------------------------------------------------------------------
// Event loop
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn run_event_loop(
    stdout: &mut io::Stdout,
    path: &Path,
    file_type: &FileType,
    initial_theme: PeekThemeName,
    rerender_on_resize: bool,
    initial_pretty: bool,
    background: Option<Rc<Cell<Background>>>,
    render_content: &dyn Fn(PeekThemeName, bool) -> Result<Vec<String>>,
) -> Result<()> {
    let mut pretty = initial_pretty;
    let content_lines = render_content(initial_theme, pretty)?;
    let mut state = ViewerState::new(path, file_type, initial_theme, content_lines, HELP_KEYS)?;

    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

    let redraw = |stdout: &mut io::Stdout, state: &ViewerState| -> Result<()> {
        let status = render_status_line(filename, state);
        state.draw(stdout, &status)
    };

    redraw(stdout, &state)?;

    loop {
        match event::read()? {
            Event::Key(key) => match state.handle_key(key) {
                KeyAction::Quit => return Ok(()),
                KeyAction::Redraw => {
                    redraw(stdout, &state)?;
                }
                KeyAction::ThemeChanged => {
                    state.content_lines = render_content(state.current_theme, pretty)?;
                    redraw(stdout, &state)?;
                }
                KeyAction::Unhandled(key) => match key.code {
                    // Raw / pretty-print toggle
                    KeyCode::Char('r') => {
                        pretty = !pretty;
                        state.content_lines = render_content(state.current_theme, pretty)?;
                        state.scroll.reset_content();
                        redraw(stdout, &state)?;
                    }
                    // Background cycling (image/SVG viewers only)
                    KeyCode::Char('b') => {
                        if let Some(ref bg_cell) = background {
                            bg_cell.set(bg_cell.get().next());
                            state.content_lines = render_content(state.current_theme, pretty)?;
                            state.scroll.reset_content();
                            redraw(stdout, &state)?;
                        }
                    }
                    _ => {}
                },
            },
            Event::Resize(_, _) => {
                if rerender_on_resize {
                    state.content_lines = render_content(state.current_theme, pretty)?;
                }
                redraw(stdout, &state)?;
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Status line
// ---------------------------------------------------------------------------

fn render_status_line(filename: &str, state: &ViewerState) -> String {
    let theme = &state.peek_theme;
    render_themed_status_line(
        &[
            (filename, theme.accent),
            (state.view_mode.label(), theme.label),
            (state.current_theme.cli_name(), theme.muted),
        ],
        &["h:help", "Tab:cycle", "t:theme", "q:quit"],
        theme,
    )
}
