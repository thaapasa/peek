use std::io::{self, Write};

use anyhow::Result;
use crossterm::{
    cursor, execute,
    event::{KeyCode, KeyEvent, KeyModifiers},
    terminal::{self, ClearType},
};

use syntect::highlighting::Color;

use crate::detect::FileType;
use crate::info::FileInfo;
use crate::input::InputSource;
use crate::theme::{ANSI_RESET_BYTES, PeekTheme, PeekThemeName, load_embedded_theme};

// ---------------------------------------------------------------------------
// View mode
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ViewMode {
    Content,
    Info,
    Help,
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

// ---------------------------------------------------------------------------
// Scroll state
// ---------------------------------------------------------------------------

pub(crate) struct ScrollState {
    content: usize,
    info: usize,
    help: usize,
}

impl ScrollState {
    fn new() -> Self {
        Self { content: 0, info: 0, help: 0 }
    }

    pub(crate) fn get(&self, mode: ViewMode) -> usize {
        match mode {
            ViewMode::Content => self.content,
            ViewMode::Info => self.info,
            ViewMode::Help => self.help,
        }
    }

    pub(crate) fn get_mut(&mut self, mode: ViewMode) -> &mut usize {
        match mode {
            ViewMode::Content => &mut self.content,
            ViewMode::Info => &mut self.info,
            ViewMode::Help => &mut self.help,
        }
    }

    pub(crate) fn reset_content(&mut self) {
        self.content = 0;
    }
}

// ---------------------------------------------------------------------------
// Key action
// ---------------------------------------------------------------------------

pub(crate) enum KeyAction {
    /// User wants to quit (q, Esc, Ctrl+C).
    Quit,
    /// View mode or scroll changed; caller should redraw.
    Redraw,
    /// Theme was cycled; caller must re-render content_lines, then redraw.
    ThemeChanged,
    /// Key not handled; caller should check viewer-specific bindings.
    Unhandled(KeyEvent),
}

// ---------------------------------------------------------------------------
// Viewer state
// ---------------------------------------------------------------------------

pub(crate) struct ViewerState {
    pub view_mode: ViewMode,
    pub current_theme: PeekThemeName,
    pub peek_theme: PeekTheme,
    pub content_lines: Vec<String>,
    pub info_lines: Vec<String>,
    pub help_lines: Vec<String>,
    pub scroll: ScrollState,
    file_info: FileInfo,
    help_keys: &'static [(&'static str, &'static str)],
}

impl ViewerState {
    pub(crate) fn new(
        source: &InputSource,
        file_type: &FileType,
        theme_name: PeekThemeName,
        content_lines: Vec<String>,
        help_keys: &'static [(&'static str, &'static str)],
    ) -> Result<Self> {
        let peek_theme = make_peek_theme(theme_name);
        let file_info = crate::info::gather(source, file_type)?;
        let info_lines = crate::info::render(&file_info, &peek_theme);
        let help_lines = render_help_with_keys(&peek_theme, theme_name, help_keys);
        Ok(Self {
            view_mode: ViewMode::Content,
            current_theme: theme_name,
            peek_theme,
            content_lines,
            info_lines,
            help_lines,
            scroll: ScrollState::new(),
            file_info,
            help_keys,
        })
    }

    /// Handle a key event for shared bindings (quit, view switching, scrolling, theme).
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> KeyAction {
        match key.code {
            // Quit
            KeyCode::Char('q') | KeyCode::Esc => KeyAction::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyAction::Quit,

            // View switching: Tab
            KeyCode::Tab => {
                self.view_mode = match self.view_mode {
                    ViewMode::Content => ViewMode::Info,
                    ViewMode::Info | ViewMode::Help => ViewMode::Content,
                };
                KeyAction::Redraw
            }

            // View switching: i
            KeyCode::Char('i') => {
                self.view_mode = ViewMode::Info;
                KeyAction::Redraw
            }

            // View switching: h / ?
            KeyCode::Char('h') | KeyCode::Char('?') => {
                self.view_mode = if self.view_mode == ViewMode::Help {
                    ViewMode::Content
                } else {
                    ViewMode::Help
                };
                KeyAction::Redraw
            }

            // Theme cycling
            KeyCode::Char('t') => {
                self.current_theme = self.current_theme.next();
                self.peek_theme = make_peek_theme(self.current_theme);
                self.info_lines = crate::info::render(&self.file_info, &self.peek_theme);
                self.help_lines = render_help_with_keys(
                    &self.peek_theme,
                    self.current_theme,
                    self.help_keys,
                );
                KeyAction::ThemeChanged
            }

            // Scrolling
            KeyCode::Up | KeyCode::Char('k') => {
                let s = self.scroll.get_mut(self.view_mode);
                *s = s.saturating_sub(1);
                KeyAction::Redraw
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = self.max_scroll();
                let s = self.scroll.get_mut(self.view_mode);
                *s = (*s + 1).min(max);
                KeyAction::Redraw
            }
            KeyCode::PageUp => {
                let rows = content_rows();
                let s = self.scroll.get_mut(self.view_mode);
                *s = s.saturating_sub(rows.saturating_sub(1));
                KeyAction::Redraw
            }
            KeyCode::PageDown | KeyCode::Char(' ') => {
                let rows = content_rows();
                let max = self.max_scroll();
                let s = self.scroll.get_mut(self.view_mode);
                *s = (*s + rows.saturating_sub(1)).min(max);
                KeyAction::Redraw
            }
            KeyCode::Home => {
                *self.scroll.get_mut(self.view_mode) = 0;
                KeyAction::Redraw
            }
            KeyCode::End => {
                *self.scroll.get_mut(self.view_mode) = self.max_scroll();
                KeyAction::Redraw
            }

            _ => KeyAction::Unhandled(key),
        }
    }

    /// Lines for the current view mode.
    pub(crate) fn current_lines(&self) -> &[String] {
        match self.view_mode {
            ViewMode::Content => &self.content_lines,
            ViewMode::Info => &self.info_lines,
            ViewMode::Help => &self.help_lines,
        }
    }

    /// Current scroll offset for the active view mode.
    pub(crate) fn current_scroll(&self) -> usize {
        self.scroll.get(self.view_mode)
    }

    /// Draw the screen with a caller-provided status line string.
    pub(crate) fn draw(&self, stdout: &mut io::Stdout, status: &str) -> Result<()> {
        draw_screen(stdout, self.current_lines(), self.current_scroll(), status)
    }

    /// Maximum scroll offset for the current view mode.
    fn max_scroll(&self) -> usize {
        self.current_lines().len().saturating_sub(content_rows())
    }
}

// ---------------------------------------------------------------------------
// Alternate screen wrapper
// ---------------------------------------------------------------------------

/// Enter the alternate screen and raw mode, run the closure, then always clean up.
pub(crate) fn with_alternate_screen(
    f: impl FnOnce(&mut io::Stdout) -> Result<()>,
) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::MoveTo(0, 0),
        cursor::Hide,
    )?;
    terminal::enable_raw_mode()?;

    let result = f(&mut stdout);

    // Always clean up, even on error
    let _ = terminal::disable_raw_mode();
    let _ = execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen);

    result
}

// ---------------------------------------------------------------------------
// Screen drawing
// ---------------------------------------------------------------------------

/// Render the screen: clear, draw visible lines, draw status bar on last row.
fn draw_screen(
    stdout: &mut io::Stdout,
    lines: &[String],
    scroll: usize,
    status: &str,
) -> Result<()> {
    let (_cols, total_rows) = terminal::size().unwrap_or((80, 24));
    let rows = (total_rows as usize).saturating_sub(1);

    // Reset all attributes before clearing so the clear doesn't
    // fill the screen with a leftover background color.
    stdout.write_all(ANSI_RESET_BYTES)?;
    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0),
    )?;

    let start = scroll.min(lines.len());
    let end = (start + rows).min(lines.len());
    for (i, line) in lines[start..end].iter().enumerate() {
        if i > 0 {
            stdout.write_all(b"\r\n")?;
        }
        stdout.write_all(line.as_bytes())?;
    }

    // Reset all attributes, then draw the status line on the last row
    stdout.write_all(ANSI_RESET_BYTES)?;
    execute!(stdout, cursor::MoveTo(0, total_rows.saturating_sub(1)))?;
    stdout.write_all(status.as_bytes())?;

    stdout.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Status line helpers
// ---------------------------------------------------------------------------

/// Build a themed status line from labeled segments and hint strings.
///
/// `segments` are shown on the left, joined by muted `│` separators.
/// `hints` are shown on the right, all in the muted color.
/// The whole line gets the theme's `selection` background.
pub(crate) fn render_themed_status_line(
    segments: &[(&str, Color)],
    hints: &[&str],
    theme: &PeekTheme,
) -> String {
    let sep = theme.paint_fg("\u{2502}", theme.muted);

    let left = segments
        .iter()
        .map(|(text, color)| theme.paint_fg(text, *color))
        .collect::<Vec<_>>()
        .join(&format!(" {sep} "));
    let left = format!(" {left}");

    let hints = hints
        .iter()
        .map(|h| theme.paint_fg(h, theme.muted))
        .collect::<Vec<_>>()
        .join("  ");
    let hints = format!("{hints} ");

    let cols = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    theme.paint_bg(&compose_status_line(&left, &hints, cols), theme.selection)
}

/// Compose a status line from left and right parts, padding or truncating to fit `cols`.
/// Drops hints first, then truncates left if still too wide.
fn compose_status_line(left: &str, hints: &str, cols: usize) -> String {
    let left_w = strip_ansi_width(left);
    let hints_w = strip_ansi_width(hints);

    if left_w + hints_w <= cols {
        // Both fit — pad the gap
        let gap = cols.saturating_sub(left_w + hints_w);
        format!("{}{}{}", left, " ".repeat(gap), hints)
    } else if left_w < cols {
        // Left fits, truncate hints to fill remaining space
        let remaining = cols.saturating_sub(left_w);
        let truncated_hints = truncate_ansi(hints, remaining);
        let hints_actual = strip_ansi_width(&truncated_hints);
        let pad = cols.saturating_sub(left_w + hints_actual);
        format!("{}{}{}", left, " ".repeat(pad), truncated_hints)
    } else {
        // Left alone overflows — truncate it, no hints
        truncate_ansi(left, cols)
    }
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

/// Truncate a string containing ANSI escapes to at most `max_width` visible characters.
pub(crate) fn truncate_ansi(s: &str, max_width: usize) -> String {
    let mut result = String::with_capacity(s.len());
    let mut width = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            result.push(c);
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
            result.push(c);
        } else {
            if width >= max_width {
                break;
            }
            result.push(c);
            width += 1;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Theme helpers
// ---------------------------------------------------------------------------

pub(crate) fn make_peek_theme(name: PeekThemeName) -> PeekTheme {
    let syntect_theme = load_embedded_theme(name.tmtheme_source());
    PeekTheme::from_syntect(&syntect_theme)
}

// ---------------------------------------------------------------------------
// Terminal helpers
// ---------------------------------------------------------------------------

pub(crate) fn terminal_rows() -> usize {
    terminal::size().map(|(_, h)| h as usize).unwrap_or(24)
}

/// Visible rows available for content (total rows minus status line).
pub(crate) fn content_rows() -> usize {
    terminal_rows().saturating_sub(1)
}

// ---------------------------------------------------------------------------
// Help renderer
// ---------------------------------------------------------------------------

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
