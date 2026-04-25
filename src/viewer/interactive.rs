use std::cell::Cell;
use std::io;
use std::rc::Rc;

use anyhow::Result;
use crossterm::event::{self, Event};

use crate::input::detect::FileType;
use crate::input::InputSource;
use crate::theme::PeekThemeName;
use crate::viewer::hex::run_hex_loop;
use crate::viewer::image::Background;
use crate::viewer::ui::{
    Action, Outcome, ViewMode, ViewerState, keys, render_themed_status_line, with_alternate_screen,
};

// ---------------------------------------------------------------------------
// Bindings for the generic interactive viewer
// ---------------------------------------------------------------------------

const ACTIONS: &[(Action, &str)] = &[
    (Action::Quit,              "Quit"),
    (Action::ScrollUp,          "Scroll up"),
    (Action::ScrollDown,        "Scroll down"),
    (Action::PageUp,            "Page up"),
    (Action::PageDown,          "Page down"),
    (Action::Top,               "Jump to top"),
    (Action::Bottom,            "Jump to bottom"),
    (Action::ToggleContentInfo, "Toggle content / file info"),
    (Action::SwitchInfo,        "File info"),
    (Action::ToggleHelp,        "Toggle help"),
    (Action::CycleTheme,        "Next theme"),
    (Action::SwitchToHex,       "Hex dump mode"),
    (Action::ToggleRawSource,   "Toggle raw / pretty"),
    (Action::CycleBackground,   "Cycle background (images)"),
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
    source: &InputSource,
    file_type: &FileType,
    theme_name: PeekThemeName,
    rerender_on_resize: bool,
    pretty: bool,
    render_content: impl Fn(PeekThemeName, bool) -> Result<Vec<String>>,
) -> Result<()> {
    view_interactive_with_bg(
        source,
        file_type,
        theme_name,
        rerender_on_resize,
        pretty,
        None,
        render_content,
    )
}

/// Interactive viewer with optional background cycling support.
/// When `background` is `Some`, the `b` key cycles the background mode.
pub fn view_interactive_with_bg(
    source: &InputSource,
    file_type: &FileType,
    theme_name: PeekThemeName,
    rerender_on_resize: bool,
    pretty: bool,
    background: Option<Rc<Cell<Background>>>,
    render_content: impl Fn(PeekThemeName, bool) -> Result<Vec<String>>,
) -> Result<()> {
    with_alternate_screen(|stdout| {
        run_event_loop(
            stdout, source, file_type, theme_name,
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
    source: &InputSource,
    file_type: &FileType,
    initial_theme: PeekThemeName,
    rerender_on_resize: bool,
    initial_pretty: bool,
    background: Option<Rc<Cell<Background>>>,
    render_content: &dyn Fn(PeekThemeName, bool) -> Result<Vec<String>>,
) -> Result<()> {
    let mut pretty = initial_pretty;
    let content_lines = render_content(initial_theme, pretty)?;
    let mut state = ViewerState::new(source, file_type, initial_theme, content_lines, ACTIONS)?;

    let name = source.name().to_string();

    let redraw = |stdout: &mut io::Stdout, state: &ViewerState| -> Result<()> {
        let status = render_status_line(&name, state);
        state.draw(stdout, &status)
    };

    redraw(stdout, &state)?;

    loop {
        match event::read()? {
            Event::Key(key) => {
                let Some(action) = keys::dispatch(key, ACTIONS) else {
                    continue;
                };
                match state.apply(action) {
                    Outcome::Quit => return Ok(()),
                    Outcome::Redraw => redraw(stdout, &state)?,
                    Outcome::RecomputeContent => {
                        state.content_lines = render_content(state.current_theme, pretty)?;
                        redraw(stdout, &state)?;
                    }
                    Outcome::Unhandled => match action {
                        Action::SwitchToHex => {
                            let line = state.scroll.get(ViewMode::Content);
                            let offset = compute_byte_offset_for_line(source, line).unwrap_or(0);
                            run_hex_loop(
                                stdout,
                                source,
                                file_type,
                                state.current_theme,
                                offset,
                                true,
                            )?;
                            redraw(stdout, &state)?;
                        }
                        Action::CycleBackground => {
                            if let Some(ref bg_cell) = background {
                                bg_cell.set(bg_cell.get().next());
                                state.content_lines = render_content(state.current_theme, pretty)?;
                                state.scroll.reset_content();
                                redraw(stdout, &state)?;
                            }
                        }
                        Action::ToggleRawSource => {
                            pretty = !pretty;
                            state.content_lines = render_content(state.current_theme, pretty)?;
                            state.scroll.reset_content();
                            redraw(stdout, &state)?;
                        }
                        _ => {}
                    },
                }
            }
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

/// Compute the byte offset in the source corresponding to a given line index
/// (0-based). Translates by counting newlines in the raw source bytes — so
/// it matches displayed lines for plain-text and syntax-highlighted views,
/// but is approximate for pretty-printed structured content.
pub(crate) fn compute_byte_offset_for_line(
    source: &InputSource,
    line: usize,
) -> Result<u64> {
    if line == 0 {
        return Ok(0);
    }
    let bytes = source.read_bytes()?;
    let mut newlines = 0usize;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            newlines += 1;
            if newlines == line {
                return Ok((i + 1) as u64);
            }
        }
    }
    Ok(bytes.len() as u64)
}

fn render_status_line(name: &str, state: &ViewerState) -> String {
    let theme = &state.peek_theme;
    render_themed_status_line(
        &[
            (name, theme.accent),
            (state.view_mode.label(), theme.label),
            (state.current_theme.cli_name(), theme.muted),
        ],
        &["h:help", "Tab:cycle", "t:theme", "q:quit"],
        theme,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdin_source(text: &str) -> InputSource {
        InputSource::Stdin {
            data: text.as_bytes().to_vec(),
        }
    }

    #[test]
    fn byte_offset_for_first_line_is_zero() {
        let s = stdin_source("alpha\nbeta\ngamma\n");
        assert_eq!(compute_byte_offset_for_line(&s, 0).unwrap(), 0);
    }

    #[test]
    fn byte_offset_after_n_newlines() {
        let s = stdin_source("alpha\nbeta\ngamma\n");
        // line 1 starts at byte 6 (after "alpha\n")
        assert_eq!(compute_byte_offset_for_line(&s, 1).unwrap(), 6);
        // line 2 starts at byte 11 (after "alpha\nbeta\n")
        assert_eq!(compute_byte_offset_for_line(&s, 2).unwrap(), 11);
    }

    #[test]
    fn byte_offset_past_eof_returns_len() {
        let s = stdin_source("a\nb\nc\n");
        let len = "a\nb\nc\n".len() as u64;
        assert_eq!(compute_byte_offset_for_line(&s, 999).unwrap(), len);
    }

    #[test]
    fn byte_offset_no_trailing_newline() {
        let s = stdin_source("first\nsecond");
        assert_eq!(compute_byte_offset_for_line(&s, 1).unwrap(), 6);
        // line 2 doesn't exist (only one newline) → returns len
        let len = "first\nsecond".len() as u64;
        assert_eq!(compute_byte_offset_for_line(&s, 2).unwrap(), len);
    }
}
