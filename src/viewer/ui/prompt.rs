//! Modal text input — overlays the status line, consumes raw keys
//! until Enter / Esc. Not a `Mode`: keeps the underlying mode visible
//! and avoids touching the existing mode-stack / scroll / cache
//! plumbing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::theme::PeekTheme;

pub(crate) struct Prompt {
    title: String,
    input: String,
    /// Byte offset into `input`, always at a UTF-8 char boundary.
    cursor: usize,
}

pub(crate) enum PromptOutcome {
    /// Prompt still open; redraw status line.
    Continue,
    /// Enter pressed; trimmed input.
    Confirmed(String),
    /// Esc / Ctrl-C; close without action.
    Cancelled,
}

impl Prompt {
    pub fn new(title: impl Into<String>, prefill: impl Into<String>) -> Self {
        let input = prefill.into();
        let cursor = input.len();
        Self {
            title: title.into(),
            input,
            cursor,
        }
    }

    #[cfg(test)]
    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> PromptOutcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => PromptOutcome::Cancelled,
            KeyCode::Char('c') if ctrl => PromptOutcome::Cancelled,
            KeyCode::Enter => {
                let final_value = self.input.trim().to_string();
                PromptOutcome::Confirmed(final_value)
            }
            KeyCode::Backspace => {
                self.delete_prev_char();
                PromptOutcome::Continue
            }
            KeyCode::Delete => {
                self.delete_next_char();
                PromptOutcome::Continue
            }
            KeyCode::Left => {
                self.move_left();
                PromptOutcome::Continue
            }
            KeyCode::Right => {
                self.move_right();
                PromptOutcome::Continue
            }
            KeyCode::Home => {
                self.cursor = 0;
                PromptOutcome::Continue
            }
            KeyCode::Char('a') if ctrl => {
                self.cursor = 0;
                PromptOutcome::Continue
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                PromptOutcome::Continue
            }
            KeyCode::Char('e') if ctrl => {
                self.cursor = self.input.len();
                PromptOutcome::Continue
            }
            // Readline kill keys: ^U cuts to start, ^K cuts to end.
            KeyCode::Char('u') if ctrl => {
                self.input.drain(..self.cursor);
                self.cursor = 0;
                PromptOutcome::Continue
            }
            KeyCode::Char('k') if ctrl => {
                self.input.truncate(self.cursor);
                PromptOutcome::Continue
            }
            KeyCode::Char(c) if !ctrl => {
                self.insert_char(c);
                PromptOutcome::Continue
            }
            _ => PromptOutcome::Continue,
        }
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    fn delete_prev_char(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = prev_char_boundary(&self.input, self.cursor);
        self.input.drain(prev..self.cursor);
        self.cursor = prev;
    }

    fn delete_next_char(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let next = next_char_boundary(&self.input, self.cursor);
        self.input.drain(self.cursor..next);
    }

    fn move_left(&mut self) {
        self.cursor = prev_char_boundary(&self.input, self.cursor);
    }

    fn move_right(&mut self) {
        self.cursor = next_char_boundary(&self.input, self.cursor);
    }

    /// Render as a status-line replacement. Caret is drawn inline
    /// (no real cursor move needed).
    pub fn render_status_line(&self, theme: &PeekTheme) -> String {
        let title = format!("{}: ", self.title);
        let painted_title = theme.paint(&title, theme.label);
        let (left, right) = self.input.split_at(self.cursor);
        let painted_left = theme.paint(left, theme.foreground);
        let painted_caret = theme.paint("\u{2581}", theme.accent);
        let painted_right = theme.paint(right, theme.foreground);
        let hint = theme.paint("  Esc:cancel  Enter:save", theme.muted);
        format!("{painted_title}{painted_left}{painted_caret}{painted_right}{hint}")
    }
}

fn prev_char_boundary(s: &str, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut i = pos - 1;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_char_boundary(s: &str, pos: usize) -> usize {
    let len = s.len();
    if pos >= len {
        return len;
    }
    let mut i = pos + 1;
    while i < len && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn insert_and_delete_round_trip() {
        let mut p = Prompt::new("Save to", "");
        for c in "abc".chars() {
            p.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(p.input(), "abc");
        p.handle_key(key(KeyCode::Backspace));
        assert_eq!(p.input(), "ab");
        p.handle_key(key(KeyCode::Backspace));
        p.handle_key(key(KeyCode::Backspace));
        assert_eq!(p.input(), "");
    }

    #[test]
    fn enter_returns_trimmed_value() {
        let mut p = Prompt::new("Save to", "  hello  ");
        match p.handle_key(key(KeyCode::Enter)) {
            PromptOutcome::Confirmed(s) => assert_eq!(s, "hello"),
            _ => panic!("expected Confirmed"),
        }
    }

    #[test]
    fn esc_cancels() {
        let mut p = Prompt::new("Save to", "anything");
        assert!(matches!(
            p.handle_key(key(KeyCode::Esc)),
            PromptOutcome::Cancelled
        ));
    }

    #[test]
    fn left_right_navigate_inside_input() {
        let mut p = Prompt::new("Save to", "abc");
        // Cursor starts at end (3). Move left twice → cursor at 1.
        p.handle_key(key(KeyCode::Left));
        p.handle_key(key(KeyCode::Left));
        // Insert 'x' at position 1: "axbc"
        p.handle_key(key(KeyCode::Char('x')));
        assert_eq!(p.input(), "axbc");
    }

    #[test]
    fn unicode_movement_and_delete() {
        // "héllo" — é is 2 bytes.
        let mut p = Prompt::new("Save to", "héllo");
        p.handle_key(key(KeyCode::Home));
        p.handle_key(key(KeyCode::Right)); // past 'h'
        p.handle_key(key(KeyCode::Delete)); // delete 'é'
        assert_eq!(p.input(), "hllo");
    }
}
