use std::io::{self, Write};

use anyhow::Result;
use crossterm::{
    cursor, execute,
    terminal::{self, ClearType},
};

use crate::info::FileInfo;
use crate::input::InputSource;
use crate::input::detect::FileType;
use crate::theme::{ANSI_RESET_BYTES, PeekTheme, PeekThemeName};

use super::help::render_help_with_keys;
use super::keys::{Action, Outcome};
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
    help_keys: &'static [(Action, &'static str)],
}

impl ViewerState {
    pub(crate) fn new(
        source: &InputSource,
        file_type: &FileType,
        theme_name: PeekThemeName,
        content_lines: Vec<String>,
        help_keys: &'static [(Action, &'static str)],
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

    // -----------------------------------------------------------------
    // Shared action handlers — invoked by `apply()`.
    // -----------------------------------------------------------------

    fn scroll_by(&mut self, delta: isize) {
        let max = self.max_scroll();
        let s = self.scroll.get_mut(self.view_mode);
        *s = if delta < 0 {
            s.saturating_sub((-delta) as usize)
        } else {
            (*s + delta as usize).min(max)
        };
    }

    fn page(&mut self, direction: isize) {
        let step = content_rows().saturating_sub(1);
        let max = self.max_scroll();
        let s = self.scroll.get_mut(self.view_mode);
        *s = if direction < 0 {
            s.saturating_sub(step)
        } else {
            (*s + step).min(max)
        };
    }

    fn jump_top(&mut self) {
        *self.scroll.get_mut(self.view_mode) = 0;
    }

    fn jump_bottom(&mut self) {
        *self.scroll.get_mut(self.view_mode) = self.max_scroll();
    }

    fn switch_to_info(&mut self) {
        self.view_mode = ViewMode::Info;
    }

    fn toggle_help(&mut self) {
        self.view_mode = if self.view_mode == ViewMode::Help {
            ViewMode::Content
        } else {
            ViewMode::Help
        };
    }

    fn toggle_content_info(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Content => ViewMode::Info,
            ViewMode::Info | ViewMode::Help => ViewMode::Content,
        };
    }

    fn cycle_theme(&mut self) {
        self.current_theme = self.current_theme.next();
        self.peek_theme = make_peek_theme(self.current_theme);
        self.info_lines = crate::info::render(&self.file_info, &self.peek_theme);
        self.help_lines = render_help_with_keys(
            &self.peek_theme,
            self.current_theme,
            self.help_keys,
        );
    }

    /// Apply a shared action. Returns `Outcome::Unhandled` for actions the
    /// state layer doesn't own (viewer-specific keys like `b`, `r`, `p`).
    pub(crate) fn apply(&mut self, action: Action) -> Outcome {
        match action {
            Action::Quit              => Outcome::Quit,
            Action::ScrollUp          => { self.scroll_by(-1); Outcome::Redraw }
            Action::ScrollDown        => { self.scroll_by(1); Outcome::Redraw }
            Action::PageUp            => { self.page(-1); Outcome::Redraw }
            Action::PageDown          => { self.page(1); Outcome::Redraw }
            Action::Top               => { self.jump_top(); Outcome::Redraw }
            Action::Bottom            => { self.jump_bottom(); Outcome::Redraw }
            Action::SwitchInfo        => { self.switch_to_info(); Outcome::Redraw }
            Action::ToggleHelp        => { self.toggle_help(); Outcome::Redraw }
            Action::ToggleContentInfo => { self.toggle_content_info(); Outcome::Redraw }
            Action::CycleTheme        => { self.cycle_theme(); Outcome::RecomputeContent }
            _                         => Outcome::Unhandled,
        }
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
