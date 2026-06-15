//! Modal text input — overlays the status line, consumes raw keys
//! until Enter / Esc. Not a `Mode`: keeps the underlying mode visible
//! and avoids touching the existing mode-stack / scroll / cache
//! plumbing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use peek_theme::PeekTheme;

pub struct Prompt {
    title: String,
    input: String,
    /// Byte offset into `input`, always at a UTF-8 char boundary.
    cursor: usize,
    /// Yes/No confirmation rather than text entry: no input field, `y` /
    /// Enter confirms (with an empty value), `n` / Esc cancels. Used to
    /// gate a slow/large op behind an explicit keystroke.
    confirm: bool,
    /// `Some` makes this a search prompt with a literal/regex toggle —
    /// the bool is the current regex state, flipped by Ctrl-R. `None` for
    /// every other prompt (no toggle, no mode hint).
    regex: Option<bool>,
}

pub enum PromptOutcome {
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
            confirm: false,
            regex: None,
        }
    }

    /// A yes/no confirmation prompt — no text field. `y` / Enter confirm
    /// (value is empty), `n` / Esc cancel.
    pub fn confirm(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            input: String::new(),
            cursor: 0,
            confirm: true,
            regex: None,
        }
    }

    /// A search prompt: text entry with a literal/regex toggle. `regex`
    /// is the starting mode — the session passes its remembered choice so
    /// the toggle sticks across searches; Ctrl-R flips it.
    pub fn search(regex: bool) -> Self {
        Self {
            title: "Search".into(),
            input: String::new(),
            cursor: 0,
            confirm: false,
            regex: Some(regex),
        }
    }

    /// Regex-toggle state — `true` only for a search prompt currently in
    /// regex mode. Read on confirm to pick the matching engine.
    pub fn is_regex(&self) -> bool {
        self.regex == Some(true)
    }

    #[cfg(test)]
    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> PromptOutcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.confirm {
            return match key.code {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => {
                    PromptOutcome::Confirmed(String::new())
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N') => PromptOutcome::Cancelled,
                KeyCode::Char('c') if ctrl => PromptOutcome::Cancelled,
                _ => PromptOutcome::Continue,
            };
        }
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
            // Toggle literal/regex on a search prompt; no-op elsewhere.
            KeyCode::Char('r') if ctrl => {
                if let Some(r) = &mut self.regex {
                    *r = !*r;
                }
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
        if self.confirm {
            let painted_title = theme.paint(&self.title, theme.label);
            let hint = theme.paint("  y:yes  n/Esc:no", theme.muted);
            return format!("{painted_title}{hint}");
        }
        // A search prompt names its engine and offers the ^R toggle; every
        // other prompt keeps the bare title + save hint.
        let (mode, hint_text) = match self.regex {
            Some(true) => (" (regex)", "  ^R:literal  Esc:cancel  Enter:search"),
            Some(false) => (" (literal)", "  ^R:regex  Esc:cancel  Enter:search"),
            None => ("", "  Esc:cancel  Enter:save"),
        };
        let title = format!("{}{mode}: ", self.title);
        let painted_title = theme.paint(&title, theme.label);
        let (left, right) = self.input.split_at(self.cursor);
        let painted_left = theme.paint(left, theme.foreground);
        let painted_caret = theme.paint("\u{2581}", theme.accent);
        let painted_right = theme.paint(right, theme.foreground);
        let hint = theme.paint(hint_text, theme.muted);
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

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn search_prompt_starts_literal_and_ctrl_r_toggles_regex() {
        let mut p = Prompt::search(false);
        assert!(!p.is_regex(), "search starts in literal mode");
        p.handle_key(ctrl('r'));
        assert!(p.is_regex(), "^R flips to regex");
        p.handle_key(ctrl('r'));
        assert!(!p.is_regex(), "^R flips back to literal");
        // A prompt seeded as regex starts there.
        assert!(
            Prompt::search(true).is_regex(),
            "seeded regex starts in regex mode"
        );
    }

    #[test]
    fn ctrl_r_is_inert_on_non_search_prompt() {
        let mut p = Prompt::new("Save to", "");
        p.handle_key(ctrl('r'));
        assert!(!p.is_regex());
        // The typed text field is untouched — ^R inserts nothing.
        assert_eq!(p.input(), "");
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
    fn confirm_y_and_enter_confirm_empty() {
        for code in [KeyCode::Char('y'), KeyCode::Char('Y'), KeyCode::Enter] {
            let mut p = Prompt::confirm("Open 2.0 GiB entry?");
            match p.handle_key(key(code)) {
                PromptOutcome::Confirmed(s) => assert!(s.is_empty(), "confirm carries no value"),
                _ => panic!("expected Confirmed for {code:?}"),
            }
        }
    }

    #[test]
    fn confirm_n_and_esc_cancel_and_text_is_inert() {
        for code in [KeyCode::Char('n'), KeyCode::Char('N'), KeyCode::Esc] {
            let mut p = Prompt::confirm("Open?");
            assert!(
                matches!(p.handle_key(key(code)), PromptOutcome::Cancelled),
                "expected Cancelled for {code:?}"
            );
        }
        // Stray text doesn't confirm or cancel — only y/n/Enter/Esc act.
        let mut p = Prompt::confirm("Open?");
        assert!(matches!(
            p.handle_key(key(KeyCode::Char('q'))),
            PromptOutcome::Continue
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
