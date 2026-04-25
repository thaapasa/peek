use std::io::{self, Write};

use anyhow::Result;
use crossterm::{
    cursor, execute,
    event::{KeyCode, KeyEvent, KeyModifiers},
    terminal::{self, ClearType},
};

use crate::info::FileInfo;
use crate::input::InputSource;
use crate::input::detect::FileType;
use crate::theme::{ANSI_RESET_BYTES, PeekTheme, PeekThemeName};

use super::help::render_help_with_keys;
use super::keys::KeyAction;
use super::{content_rows, make_peek_theme};

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

            // Hex-mode toggle
            KeyCode::Char('x') => KeyAction::SwitchToHex,

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
