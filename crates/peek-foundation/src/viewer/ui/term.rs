//! Terminal interaction: the alternate-screen + raw-mode session guard,
//! terminal-size queries with their test override, and the content-rows
//! helper (rows minus the status line).

use std::io;

use anyhow::Result;
use crossterm::{cursor, execute, terminal};

/// Enter the alternate screen and raw mode, run the closure, then always clean up.
///
/// Cleanup runs via `Drop`, so a panic inside `f` still restores the
/// terminal — without the guard, an unwinding panic would leave the
/// user's shell in raw-mode + alternate-screen, which is unrecoverable
/// without `reset(1)`.
pub fn with_alternate_screen(f: impl FnOnce(&mut io::Stdout) -> Result<()>) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::MoveTo(0, 0),
        cursor::Hide,
    )?;
    terminal::enable_raw_mode()?;

    let _guard = TerminalGuard;
    f(&mut stdout)
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show, terminal::LeaveAlternateScreen);
    }
}

pub fn terminal_rows() -> usize {
    #[cfg(any(test, feature = "testing"))]
    if let Some((_, rows)) = test_term_override::get() {
        return rows;
    }
    terminal::size().map(|(_, h)| h as usize).unwrap_or(24)
}

pub fn terminal_cols() -> usize {
    #[cfg(any(test, feature = "testing"))]
    if let Some((cols, _)) = test_term_override::get() {
        return cols;
    }
    terminal::size().map(|(w, _)| w as usize).unwrap_or(80)
}

#[cfg(any(test, feature = "testing"))]
pub mod test_term_override {
    use std::cell::Cell;

    thread_local! {
        static OVERRIDE: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
    }

    pub fn get() -> Option<(usize, usize)> {
        OVERRIDE.with(|c| c.get())
    }

    /// RAII guard pinning `terminal_cols()` / `terminal_rows()` to fixed
    /// values for the lifetime of the returned scope. Clears the override
    /// on drop (including panic) so tests can't leak the pin to siblings.
    pub struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            OVERRIDE.with(|c| c.set(None));
        }
    }

    pub fn pin(cols: usize, rows: usize) -> Guard {
        OVERRIDE.with(|c| c.set(Some((cols, rows))));
        Guard
    }
}

/// Visible rows available for content (total rows minus status line).
pub fn content_rows() -> usize {
    terminal_rows().saturating_sub(1)
}
