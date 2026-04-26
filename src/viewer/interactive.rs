use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event};

use crate::info::RenderOptions;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::theme::PeekThemeName;
use crate::viewer::modes::Mode;
use crate::viewer::ui::{
    Outcome, ViewerState, render_themed_status_line, with_alternate_screen,
};

/// Run the interactive viewer for a given list of view modes.
///
/// Enters the alternate screen, drives the event loop, and routes key
/// events through the active mode's scroll/handle dispatch before falling
/// through to global actions.
///
/// Modes are owned for the duration of the call. The first mode in the
/// list is the initial active view; `Tab`, `i`, `h`, `x` switch among
/// modes by id (Info, Help, Hex).
pub fn run(
    source: &InputSource,
    detected: &Detected,
    theme_name: PeekThemeName,
    render_opts: RenderOptions,
    modes: Vec<Box<dyn Mode>>,
) -> Result<()> {
    with_alternate_screen(|stdout| {
        event_loop(stdout, source, detected, theme_name, render_opts, modes)
    })
}

fn event_loop(
    stdout: &mut io::Stdout,
    source: &InputSource,
    detected: &Detected,
    theme_name: PeekThemeName,
    render_opts: RenderOptions,
    modes: Vec<Box<dyn Mode>>,
) -> Result<()> {
    let mut state = ViewerState::new(source, detected, theme_name, render_opts, modes)?;
    let name = source.name().to_string();

    redraw(stdout, &mut state, &name)?;

    loop {
        // Timer-driven modes (Animation) wake the loop with a deadline;
        // everything else uses a long block.
        let timeout = state
            .active_next_tick()
            .unwrap_or(Duration::from_secs(86_400));
        if !event::poll(timeout)? {
            // Timeout: tick the active mode (e.g. advance animation frame).
            if state.tick_active() {
                state.invalidate_active();
                redraw(stdout, &mut state, &name)?;
            }
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                let Some(action) = state.dispatch_key(key) else {
                    continue;
                };

                // 1. Active mode's scroll handler (byte-based for hex etc.).
                if state.try_active_scroll(action) {
                    state.invalidate_active();
                    redraw(stdout, &mut state, &name)?;
                    continue;
                }

                // 2. Active mode's local handler (toggle pretty, cycle bg, etc.).
                if state.try_active_handle(action) {
                    state.invalidate_active();
                    redraw(stdout, &mut state, &name)?;
                    continue;
                }

                // 3. Global dispatch (scroll-by-line, mode switching, theme).
                match state.apply(action) {
                    Outcome::Quit => return Ok(()),
                    Outcome::Redraw => redraw(stdout, &mut state, &name)?,
                    Outcome::Unhandled => {}
                }
            }
            Event::Resize(_, _) => {
                state.handle_resize();
                redraw(stdout, &mut state, &name)?;
            }
            _ => {}
        }
    }
}

fn redraw(stdout: &mut io::Stdout, state: &mut ViewerState, name: &str) -> Result<()> {
    state.ensure_active_rendered()?;
    let status = render_status_line(name, state);
    state.draw(stdout, &status)
}

fn render_status_line(name: &str, state: &ViewerState) -> String {
    let theme = &state.peek_theme;
    let mode_label: &str = state.active_label();
    let mode_segs = state.active_status_segments();

    let mut segs: Vec<(&str, syntect::highlighting::Color)> =
        vec![(name, theme.accent), (mode_label, theme.label)];
    for (s, c) in &mode_segs {
        segs.push((s.as_str(), *c));
    }
    segs.push((state.current_theme.cli_name(), theme.muted));

    let mut hints: Vec<&str> = state.active_status_hints();
    hints.extend_from_slice(&["h:help", "Tab:cycle", "t:theme", "q:quit"]);

    render_themed_status_line(&segs, &hints, theme)
}
