use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};

use super::render::{self, TermSize};
use super::ImageMode;

/// Interactive image viewer with resize support.
///
/// Enters alternate screen, renders the image to fit the terminal,
/// and re-renders on resize. Exits on q/Esc/Ctrl+C.
pub fn view_interactive(
    path: &Path,
    mode: ImageMode,
    forced_width: u32,
) -> Result<()> {
    let mut stdout = io::stdout();

    // Enter alternate screen, hide cursor, enable raw mode
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::Hide,
    )?;
    terminal::enable_raw_mode()?;

    let result = run_event_loop(&mut stdout, path, mode, forced_width);

    // Always restore terminal state, even on error
    terminal::disable_raw_mode()?;
    execute!(
        stdout,
        cursor::Show,
        terminal::LeaveAlternateScreen,
    )?;

    result
}

fn run_event_loop(
    stdout: &mut io::Stdout,
    path: &Path,
    mode: ImageMode,
    forced_width: u32,
) -> Result<()> {
    // Initial render
    draw(stdout, path, mode, forced_width)?;

    loop {
        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) => {
                return Ok(());
            }
            Event::Resize(_, _) => {
                draw(stdout, path, mode, forced_width)?;
            }
            _ => {}
        }
    }
}

fn draw(
    stdout: &mut io::Stdout,
    path: &Path,
    mode: ImageMode,
    forced_width: u32,
) -> Result<()> {
    let term = TermSize::detect();

    let lines = render::load_and_render(path, mode, forced_width, term)?;

    // Clear screen and move cursor to top-left
    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0),
    )?;

    // Write rendered lines
    for line in &lines {
        stdout.write_all(line.as_bytes())?;
        stdout.write_all(b"\r\n")?;
    }

    stdout.flush()?;
    Ok(())
}
