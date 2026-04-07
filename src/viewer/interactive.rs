use std::cell::Cell;
use std::io::{self, Write};
use std::path::Path;
use std::rc::Rc;

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};

use crate::detect::FileType;
use crate::theme::{PeekTheme, PeekThemeName, load_embedded_theme};
use crate::viewer::image::Background;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ViewMode {
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
    let mut stdout = io::stdout();

    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::MoveTo(0, 0),
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
        background,
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
    let mut view_mode = ViewMode::Content;
    let mut current_theme = initial_theme;
    let mut peek_theme = make_peek_theme(current_theme);
    let mut pretty = initial_pretty;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?");

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

    let redraw = |stdout: &mut io::Stdout,
                  mode: ViewMode,
                  scroll: usize,
                  content: &[String],
                  info: &[String],
                  help: &[String],
                  theme: &PeekTheme,
                  theme_name: PeekThemeName|
     -> Result<()> {
        let status = render_status_line(filename, mode, theme_name, theme);
        draw(stdout, mode, content, info, help, scroll, &status)
    };

    redraw(
        stdout, view_mode, content_scroll,
        &content_lines, &info_lines, &help_lines,
        &peek_theme, current_theme,
    )?;

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
                    redraw(
                        stdout, view_mode, scroll,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme,
                    )?;
                }
                KeyCode::Char('i') => {
                    if view_mode != ViewMode::Info {
                        view_mode = ViewMode::Info;
                        redraw(
                            stdout, view_mode, info_scroll,
                            &content_lines, &info_lines, &help_lines,
                            &peek_theme, current_theme,
                        )?;
                    }
                }
                KeyCode::Char('h') | KeyCode::Char('?') => {
                    view_mode = if view_mode == ViewMode::Help {
                        ViewMode::Content
                    } else {
                        ViewMode::Help
                    };
                    let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                    redraw(
                        stdout, view_mode, scroll,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme,
                    )?;
                }

                // Theme cycling
                KeyCode::Char('t') => {
                    current_theme = current_theme.next();
                    peek_theme = make_peek_theme(current_theme);
                    content_lines = render_content(current_theme, pretty)?;
                    info_lines = crate::info::render(&file_info, &peek_theme);
                    help_lines = render_help_lines(&peek_theme, current_theme);
                    let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                    redraw(
                        stdout, view_mode, scroll,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme,
                    )?;
                }

                // Raw / pretty-print toggle
                KeyCode::Char('r') => {
                    pretty = !pretty;
                    content_lines = render_content(current_theme, pretty)?;
                    content_scroll = 0;
                    redraw(
                        stdout, view_mode, content_scroll,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme,
                    )?;
                }

                // Background cycling (image/SVG viewers only)
                KeyCode::Char('b') => {
                    if let Some(ref bg_cell) = background {
                        bg_cell.set(bg_cell.get().next());
                        content_lines = render_content(current_theme, pretty)?;
                        content_scroll = 0;
                        redraw(
                            stdout, view_mode, content_scroll,
                            &content_lines, &info_lines, &help_lines,
                            &peek_theme, current_theme,
                        )?;
                    }
                }

                // Scrolling
                KeyCode::Up | KeyCode::Char('k') => {
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = s.saturating_sub(1);
                    redraw(
                        stdout, view_mode, *s,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme,
                    )?;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let lines = lines_for(view_mode, &content_lines, &info_lines, &help_lines);
                    let rows = content_rows();
                    let max = lines.len().saturating_sub(rows);
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = (*s + 1).min(max);
                    redraw(
                        stdout, view_mode, *s,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme,
                    )?;
                }
                KeyCode::PageUp => {
                    let rows = content_rows();
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = s.saturating_sub(rows.saturating_sub(1));
                    redraw(
                        stdout, view_mode, *s,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme,
                    )?;
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    let lines = lines_for(view_mode, &content_lines, &info_lines, &help_lines);
                    let rows = content_rows();
                    let max = lines.len().saturating_sub(rows);
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = (*s + rows.saturating_sub(1)).min(max);
                    redraw(
                        stdout, view_mode, *s,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme,
                    )?;
                }
                KeyCode::Home => {
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = 0;
                    redraw(
                        stdout, view_mode, 0,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme,
                    )?;
                }
                KeyCode::End => {
                    let lines = lines_for(view_mode, &content_lines, &info_lines, &help_lines);
                    let rows = content_rows();
                    let max = lines.len().saturating_sub(rows);
                    let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                    *s = max;
                    redraw(
                        stdout, view_mode, max,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme,
                    )?;
                }

                _ => {}
            },
            Event::Resize(_, _) => {
                if rerender_on_resize {
                    content_lines = render_content(current_theme, pretty)?;
                }
                let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                redraw(
                    stdout, view_mode, scroll,
                    &content_lines, &info_lines, &help_lines,
                    &peek_theme, current_theme,
                )?;
            }
            _ => {}
        }
    }
}

impl ViewMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            ViewMode::Content => "Content",
            ViewMode::Info => "Info",
            ViewMode::Help => "Help",
        }
    }
}

fn render_status_line(
    filename: &str,
    mode: ViewMode,
    theme_name: PeekThemeName,
    theme: &PeekTheme,
) -> String {
    use syntect::highlighting::Color;

    // Foreground-only escape that won't reset the background
    let fg = |text: &str, color: Color| -> String {
        format!("\x1b[38;2;{};{};{}m{}", color.r, color.g, color.b, text)
    };

    let left = format!(
        " {} {} {} {} {}",
        fg(filename, theme.accent),
        fg("\u{2502}", theme.muted),
        fg(mode.label(), theme.label),
        fg("\u{2502}", theme.muted),
        fg(theme_name.cli_name(), theme.muted),
    );

    let hints = format!(
        "{}  {}  {}  {} ",
        fg("h:help", theme.muted),
        fg("Tab:cycle", theme.muted),
        fg("t:theme", theme.muted),
        fg("q:quit", theme.muted),
    );

    // Compute visible widths (strip ANSI escapes for padding)
    let left_visible = strip_ansi_width(&left);
    let hints_visible = strip_ansi_width(&hints);
    let cols = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);

    let gap = cols.saturating_sub(left_visible + hints_visible);
    let padding = " ".repeat(gap);

    let bg = theme.selection;
    format!(
        "\x1b[48;2;{};{};{}m{}{}{}\x1b[0m",
        bg.r, bg.g, bg.b,
        left, padding, hints,
    )
}

/// Count the visible character width of a string, ignoring ANSI escape sequences.
pub(crate) fn strip_ansi_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            width += 1;
        }
    }
    width
}

pub(crate) fn make_peek_theme(name: PeekThemeName) -> PeekTheme {
    let syntect_theme = load_embedded_theme(name.tmtheme_source());
    PeekTheme::from_syntect(&syntect_theme)
}

pub(crate) fn terminal_rows() -> usize {
    terminal::size().map(|(_, h)| h as usize).unwrap_or(24)
}

pub(crate) fn current_scroll(
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

pub(crate) fn scroll_mut<'a>(
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

pub(crate) fn lines_for<'a>(
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

/// Visible rows available for content (total rows minus status line).
pub(crate) fn content_rows() -> usize {
    terminal_rows().saturating_sub(1)
}

pub(crate) fn draw(
    stdout: &mut io::Stdout,
    view_mode: ViewMode,
    content_lines: &[String],
    info_lines: &[String],
    help_lines: &[String],
    scroll: usize,
    status: &str,
) -> Result<()> {
    let (_cols, total_rows) = terminal::size().unwrap_or((80, 24));
    let rows = (total_rows as usize).saturating_sub(1); // reserve last row for status

    // Reset all attributes before clearing so the clear doesn't
    // fill the screen with a leftover background color.
    stdout.write_all(b"\x1b[0m")?;
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

    // Reset all attributes, then draw the status line on the last row
    stdout.write_all(b"\x1b[0m")?;
    execute!(stdout, cursor::MoveTo(0, total_rows.saturating_sub(1)))?;
    stdout.write_all(status.as_bytes())?;

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
    ("b", "Cycle background (images)"),
];

fn render_help_lines(theme: &PeekTheme, current_theme: PeekThemeName) -> Vec<String> {
    render_help_with_keys(theme, current_theme, HELP_KEYS)
}

pub(crate) fn render_help_with_keys(
    theme: &PeekTheme,
    current_theme: PeekThemeName,
    keys: &[(&str, &str)],
) -> Vec<String> {
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

    for (key, desc) in keys {
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
