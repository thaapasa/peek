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
use crate::theme::{PeekTheme, PeekThemeName, load_embedded_theme};

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Content,
    Info,
    Help,
}

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
    let mut stdout = io::stdout();

    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::Hide,
    )?;
    terminal::enable_raw_mode()?;

    let result = run_event_loop(
        &mut stdout,
        path,
        file_type,
        theme_name,
        rerender_on_resize,
        pretty,
        &render_content,
    );

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
    initial_theme: PeekThemeName,
    rerender_on_resize: bool,
    initial_pretty: bool,
    render_content: &dyn Fn(PeekThemeName, bool) -> Result<Vec<String>>,
) -> Result<()> {
    let mut view_mode = ViewMode::Content;
    let mut current_theme = initial_theme;
    let mut peek_theme = make_peek_theme(current_theme);
    let mut pretty = initial_pretty;

    // Render initial content
    let mut content_lines = render_content(current_theme, pretty)?;

    // Pre-compute file info and help lines
    let file_info = crate::info::gather(path, file_type)?;
    let mut info_lines = crate::info::render(&file_info, &peek_theme);
    let mut help_lines = render_help_lines(&peek_theme, current_theme);

    // Per-view scroll offsets
    let mut content_scroll: usize = 0;
    let mut info_scroll: usize = 0;
    let mut help_scroll: usize = 0;

    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, content_scroll)?;

    loop {
        match event::read()? {
            Event::Key(key) => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(());
                }

                // View switching
                KeyCode::Tab => {
                    view_mode = match view_mode {
                        ViewMode::Content => ViewMode::Info,
                        ViewMode::Info | ViewMode::Help => ViewMode::Content,
                    };
                    let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, scroll)?;
                }
                KeyCode::Char('i') => {
                    if view_mode != ViewMode::Info {
                        view_mode = ViewMode::Info;
                        draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, info_scroll)?;
                    }
                }
                KeyCode::Char('h') | KeyCode::Char('?') => {
                    view_mode = if view_mode == ViewMode::Help {
                        ViewMode::Content
                    } else {
                        ViewMode::Help
                    };
                    let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, scroll)?;
                }

                // Theme cycling
                KeyCode::Char('t') => {
                    current_theme = current_theme.next();
                    peek_theme = make_peek_theme(current_theme);
                    content_lines = render_content(current_theme, pretty)?;
                    info_lines = crate::info::render(&file_info, &peek_theme);
                    help_lines = render_help_lines(&peek_theme, current_theme);
                    let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, scroll)?;
                }

                // Raw / pretty-print toggle
                KeyCode::Char('r') => {
                    pretty = !pretty;
                    content_lines = render_content(current_theme, pretty)?;
                    content_scroll = 0;
                    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, content_scroll)?;
                }

                // Scrolling
                KeyCode::Up | KeyCode::Char('k') => {
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = s.saturating_sub(1);
                    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, *s)?;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let lines = lines_for(view_mode, &content_lines, &info_lines, &help_lines);
                    let rows = terminal_rows();
                    let max = lines.len().saturating_sub(rows);
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = (*s + 1).min(max);
                    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, *s)?;
                }
                KeyCode::PageUp => {
                    let rows = terminal_rows();
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = s.saturating_sub(rows.saturating_sub(1));
                    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, *s)?;
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    let lines = lines_for(view_mode, &content_lines, &info_lines, &help_lines);
                    let rows = terminal_rows();
                    let max = lines.len().saturating_sub(rows);
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = (*s + rows.saturating_sub(1)).min(max);
                    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, *s)?;
                }
                KeyCode::Home => {
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = 0;
                    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, 0)?;
                }
                KeyCode::End => {
                    let lines = lines_for(view_mode, &content_lines, &info_lines, &help_lines);
                    let rows = terminal_rows();
                    let max = lines.len().saturating_sub(rows);
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = max;
                    draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, max)?;
                }

                _ => {}
            },
            Event::Resize(_, _) => {
                if rerender_on_resize {
                    content_lines = render_content(current_theme, pretty)?;
                }
                let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                draw(stdout, view_mode, &content_lines, &info_lines, &help_lines, scroll)?;
            }
            _ => {}
        }
    }
}

fn make_peek_theme(name: PeekThemeName) -> PeekTheme {
    let syntect_theme = load_embedded_theme(name.tmtheme_source());
    PeekTheme::from_syntect(&syntect_theme)
}

fn terminal_rows() -> usize {
    terminal::size().map(|(_, h)| h as usize).unwrap_or(24)
}

fn current_scroll(
    mode: ViewMode,
    content: usize,
    info: usize,
    help: usize,
) -> usize {
    match mode {
        ViewMode::Content => content,
        ViewMode::Info => info,
        ViewMode::Help => help,
    }
}

fn scroll_mut<'a>(
    mode: ViewMode,
    content: &'a mut usize,
    info: &'a mut usize,
    help: &'a mut usize,
) -> &'a mut usize {
    match mode {
        ViewMode::Content => content,
        ViewMode::Info => info,
        ViewMode::Help => help,
    }
}

fn lines_for<'a>(
    mode: ViewMode,
    content: &'a [String],
    info: &'a [String],
    help: &'a [String],
) -> &'a [String] {
    match mode {
        ViewMode::Content => content,
        ViewMode::Info => info,
        ViewMode::Help => help,
    }
}

fn draw(
    stdout: &mut io::Stdout,
    view_mode: ViewMode,
    content_lines: &[String],
    info_lines: &[String],
    help_lines: &[String],
    scroll: usize,
) -> Result<()> {
    let rows = terminal_rows();

    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0),
    )?;

    let lines = lines_for(view_mode, content_lines, info_lines, help_lines);
    let start = scroll.min(lines.len());
    let end = (start + rows).min(lines.len());
    for (i, line) in lines[start..end].iter().enumerate() {
        if i > 0 {
            stdout.write_all(b"\r\n")?;
        }
        stdout.write_all(line.as_bytes())?;
    }

    stdout.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Inline help renderer
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
];

fn render_help_lines(theme: &PeekTheme, current_theme: PeekThemeName) -> Vec<String> {
    let mut lines = Vec::new();

    // Section header
    let rule = "\u{2500}".repeat(28);
    lines.push(format!(
        "{} {} {}",
        theme.paint_muted("\u{2500}\u{2500}"),
        theme.paint_heading("Keyboard Shortcuts"),
        theme.paint_muted(&rule),
    ));

    // Key overhead for alignment (ANSI codes in paint_label)
    let sample_painted = theme.paint_label("x");
    let overhead = sample_painted.len() - 1;
    let key_width = 14 + overhead;

    for (key, desc) in HELP_KEYS {
        lines.push(format!(
            "  {:<width$}{}",
            theme.paint_label(key),
            theme.paint_muted(desc),
            width = key_width,
        ));
    }

    lines.push(String::new());

    // Theme info
    let rule2 = "\u{2500}".repeat(35);
    lines.push(format!(
        "{} {} {}",
        theme.paint_muted("\u{2500}\u{2500}"),
        theme.paint_heading("Theme"),
        theme.paint_muted(&rule2),
    ));
    lines.push(format!(
        "  {:<width$}{}",
        theme.paint_label("Active"),
        theme.paint_value(current_theme.cli_name()),
        width = key_width,
    ));

    lines
}
