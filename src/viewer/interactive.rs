use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};

use crate::detect::FileType;
use crate::theme::PeekTheme;

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Content,
    Info,
}

/// Generic interactive viewer with alternate screen, event loop, and
/// Tab/i view switching between content and file info.
///
/// `draw_content` is called whenever the content view needs to be rendered
/// (initial draw and on resize). It receives stdout and should write
/// ANSI-escaped lines terminated with `\r\n`.
pub fn view_interactive(
    path: &Path,
    file_type: &FileType,
    peek_theme: &PeekTheme,
    draw_content: impl Fn(&mut io::Stdout) -> Result<()>,
) -> Result<()> {
    let mut stdout = io::stdout();

    // Enter alternate screen, hide cursor, enable raw mode
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::Hide,
    )?;
    terminal::enable_raw_mode()?;

    let result = run_event_loop(&mut stdout, path, file_type, peek_theme, &draw_content);

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
    file_type: &FileType,
    peek_theme: &PeekTheme,
    draw_content: &dyn Fn(&mut io::Stdout) -> Result<()>,
) -> Result<()> {
    let mut view_mode = ViewMode::Content;

    // Pre-compute file info lines (they don't change on resize)
    let file_info = crate::info::gather(path, file_type)?;
    let info_lines = crate::info::render(&file_info, peek_theme);

    // Initial render
    draw(stdout, view_mode, draw_content, &info_lines)?;

    loop {
        match event::read()? {
            Event::Key(key) => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(());
                }
                KeyCode::Tab => {
                    view_mode = match view_mode {
                        ViewMode::Content => ViewMode::Info,
                        ViewMode::Info => ViewMode::Content,
                    };
                    draw(stdout, view_mode, draw_content, &info_lines)?;
                }
                KeyCode::Char('i') => {
                    if view_mode != ViewMode::Info {
                        view_mode = ViewMode::Info;
                        draw(stdout, view_mode, draw_content, &info_lines)?;
                    }
                }
                _ => {}
            },
            Event::Resize(_, _) => {
                draw(stdout, view_mode, draw_content, &info_lines)?;
            }
            _ => {}
        }
    }
}

fn draw(
    stdout: &mut io::Stdout,
    view_mode: ViewMode,
    draw_content: &dyn Fn(&mut io::Stdout) -> Result<()>,
    info_lines: &[String],
) -> Result<()> {
    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0),
    )?;

    match view_mode {
        ViewMode::Content => {
            draw_content(stdout)?;
        }
        ViewMode::Info => {
            for line in info_lines {
                stdout.write_all(line.as_bytes())?;
                stdout.write_all(b"\r\n")?;
            }
        }
    }

    stdout.flush()?;
    Ok(())
}
