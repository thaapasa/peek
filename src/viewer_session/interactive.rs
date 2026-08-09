use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use peek_detect::Detected;
use peek_foundation::info::RenderOptions;
use peek_foundation::viewer::modes::{Mode, ModeId};
use peek_foundation::viewer::ui::{
    Action, Outcome, render_themed_status_line, with_alternate_screen,
};
use peek_io::InputSource;
use peek_theme::{PeekThemeName, StyleMode};

use crate::viewer_session::{ModeBuilder, ViewerState};

/// Run the interactive viewer for a given list of view modes.
///
/// Enters the alternate screen, drives the event loop, and routes key
/// events through the active mode's scroll/handle dispatch before falling
/// through to global actions.
///
/// Modes are owned for the duration of the call. The first mode in the
/// list is the initial active view; `Tab`, `i`, `h`, `x` switch among
/// modes by id (Info, Help, Hex).
#[allow(clippy::too_many_arguments)]
pub fn run(
    source: InputSource,
    detected: Detected,
    theme_name: PeekThemeName,
    style_mode: StyleMode,
    render_opts: RenderOptions,
    modes: Vec<Box<dyn Mode>>,
    mode_builder: ModeBuilder,
    no_tempfile: bool,
    access: super::Access,
    deferred: Option<super::Deferred>,
) -> Result<()> {
    with_alternate_screen(|stdout| {
        event_loop(
            stdout,
            source,
            detected,
            theme_name,
            style_mode,
            render_opts,
            modes,
            mode_builder,
            no_tempfile,
            access,
            deferred,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn event_loop(
    stdout: &mut io::Stdout,
    source: InputSource,
    detected: Detected,
    theme_name: PeekThemeName,
    style_mode: StyleMode,
    render_opts: RenderOptions,
    modes: Vec<Box<dyn Mode>>,
    mode_builder: ModeBuilder,
    no_tempfile: bool,
    access: super::Access,
    deferred: Option<super::Deferred>,
) -> Result<()> {
    let name = source.name().to_string();
    let mut state = ViewerState::new(
        source,
        detected,
        theme_name,
        style_mode,
        render_opts,
        modes,
        mode_builder,
        no_tempfile,
        access,
        deferred,
    )?;

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

        // Drain every queued event in one batch, redraw once at the end.
        // Coalescing identical consecutive actions stops a buffered key
        // repeat (terminal auto-repeat fills the queue faster than draws
        // happen) from firing extra actions after the user releases the
        // key — without this, a held `j` followed by release keeps
        // scrolling until the buffer drains.
        let mut needs_redraw = false;
        let mut last_action: Option<Action> = None;
        let mut event_opt: Option<Event> = Some(event::read()?);

        while let Some(ev) = event_opt {
            match ev {
                // Windows console emits both Press and Release events;
                // Unix only emits Press. Drop Release (and any non-press
                // kinds) so a single key tap doesn't fire its action twice
                // — otherwise `h` toggles Help open then immediately shut.
                Event::Key(key)
                    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {}
                Event::Key(key) => {
                    // Modal prompt: keys go straight to the input,
                    // skipping Action dispatch and coalescing (the
                    // same char twice is meaningful while typing).
                    if state.prompt_active() {
                        if state.handle_prompt_key(key)? {
                            needs_redraw = true;
                        }
                        last_action = None;
                    } else if let Some(action) = state.dispatch_key(key)
                        && last_action != Some(action)
                    {
                        match dispatch_action(&mut state, action)? {
                            ActionOutcome::Quit => return Ok(()),
                            ActionOutcome::Changed => needs_redraw = true,
                            ActionOutcome::Unchanged => {}
                        }
                        last_action = Some(action);
                    }
                }
                Event::Resize(_, _) => {
                    state.handle_resize();
                    needs_redraw = true;
                }
                _ => {}
            }
            // Pull the next queued event without blocking; stop when the
            // OS input buffer is empty.
            event_opt = if event::poll(Duration::ZERO)? {
                Some(event::read()?)
            } else {
                None
            };
        }

        if needs_redraw {
            redraw(stdout, &mut state, &name)?;
        }
    }
}

enum ActionOutcome {
    Quit,
    Changed,
    Unchanged,
}

/// Single-action dispatch: scroll handler → local handler → global. Mirrors
/// the previous inline order; lifted out so the batched event loop can
/// reuse it for every event in the drained queue.
fn dispatch_action(state: &mut ViewerState, action: Action) -> Result<ActionOutcome> {
    if state.try_active_scroll(action) {
        state.invalidate_active();
        return Ok(ActionOutcome::Changed);
    }
    if state.try_active_handle(action) {
        state.invalidate_active();
        return Ok(ActionOutcome::Changed);
    }
    Ok(match state.apply(action)? {
        Outcome::Quit => ActionOutcome::Quit,
        Outcome::Redraw => ActionOutcome::Changed,
        Outcome::Unhandled => ActionOutcome::Unchanged,
    })
}

fn redraw(stdout: &mut io::Stdout, state: &mut ViewerState, _root_name: &str) -> Result<()> {
    state.ensure_active_rendered()?;
    let status = if let Some(prompt) = state.active_prompt() {
        // Reuse the selection-coloured status background so the
        // prompt visually slots in.
        let theme = &state.peek_theme;
        theme.paint_bg(&prompt.render_status_line(theme), theme.selection)
    } else {
        render_status_line(state)
    };
    state.draw(stdout, &status)
}

fn render_status_line(state: &mut ViewerState) -> String {
    let flash = state.take_flash();
    let mode_label: String = state.active_label().to_string();
    let mode_segs = state.active_status_segments();
    let hints_owned: Vec<&'static str> = state.active_status_hints();
    let theme_name = state.current_theme.cli_name();
    let color_mode_name = state.peek_theme.style_mode.cli_name();
    // Breadcrumb: at depth 1, just the source name (matches the
    // single-session look). At depth > 1, every frame joined with
    // " > " so the user can see how deep they've drilled.
    let breadcrumb = state.breadcrumb().join(" > ");
    let has_warnings = !state.frame().file_info.warnings.is_empty();
    let deferred_hint = state.deferred_hint();
    // Surface human-size mode only where a size column exists (listings)
    // and only when it's on — default byte display stays unannounced.
    let show_human_sizes = state.active_mode_id() == ModeId::Listing && state.human_sizes();
    let theme = &state.peek_theme;

    // Prefix the breadcrumb with a warning `!` when the active frame's info
    // section has surfaced warnings (e.g. extension/MIME mismatch). Passed as
    // a plain `lead` marker, not pre-painted into the segment text — the status
    // renderer sanitizes every segment, so embedded SGR would show as literal
    // `␛[…m` glyphs.
    let warn_lead = has_warnings.then_some(("!", theme.warning));

    let mut segs: Vec<(&str, syntect::highlighting::Color)> = vec![
        (breadcrumb.as_str(), theme.accent),
        (mode_label.as_str(), theme.label),
    ];
    for (s, c) in &mode_segs {
        segs.push((s.as_str(), *c));
    }
    if let Some(msg) = flash.as_deref() {
        segs.push((msg, theme.warning));
    }
    // Deferred-decompress prompt: surface the load hint where flash sits,
    // so a freshly-opened big `.xz` shows "Enter to decompress" up front.
    if let Some(msg) = deferred_hint.as_deref() {
        segs.push((msg, theme.warning));
    }
    if show_human_sizes {
        segs.push(("human sizes", theme.muted));
    }
    segs.push((theme_name, theme.muted));
    // Only surface color mode when it's been changed off the default —
    // keeps the status line uncluttered for the common case.
    if theme.style_mode != StyleMode::default() {
        segs.push((color_mode_name, theme.muted));
    }

    let mut hints: Vec<&str> = hints_owned;
    hints.extend_from_slice(&["h:help", "Tab:cycle", "t:theme", "q:quit"]);

    render_themed_status_line(warn_lead, &segs, &hints, theme)
}
