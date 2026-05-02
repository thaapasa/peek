//! Print-mode output.
//!
//! Write-once, non-interactive stdout sink used for `--print`, piped runs
//! (stdout is not a TTY), and `--info`. Each `Viewer` impl writes its
//! whole rendering through a `PrintOutput` and returns; there's no event
//! loop here.
//!
//! For the interactive TTY path (alternate screen, mode stack, key
//! dispatch, animation ticks), see [`crate::viewer::interactive`].

use std::io::{self, Write};

use anyhow::Result;

/// Non-interactive stdout writer for the print path.
///
/// Construct with [`PrintOutput::stdout`], emit content with
/// [`write_line`](Self::write_line) / [`write_str`](Self::write_str),
/// finalize with [`finish`](Self::finish) (flush).
pub struct PrintOutput {
    stdout: io::Stdout,
}

impl PrintOutput {
    /// Wrap process stdout for line-by-line print output.
    pub fn stdout() -> Self {
        Self {
            stdout: io::stdout(),
        }
    }

    /// Write a line of text to the output.
    pub fn write_line(&mut self, line: &str) -> Result<()> {
        writeln!(self.stdout, "{line}")?;
        Ok(())
    }

    /// Write raw text (no trailing newline).
    pub fn write_str(&mut self, text: &str) -> Result<()> {
        write!(self.stdout, "{text}")?;
        Ok(())
    }

    /// Finalize output: flush stdout.
    pub fn finish(mut self) -> Result<()> {
        self.stdout.flush()?;
        Ok(())
    }
}
