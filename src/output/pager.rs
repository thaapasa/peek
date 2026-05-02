use std::fmt::Write as FmtWrite;
use std::io::{self, Write};

use anyhow::Result;

/// Output abstraction: pager or direct stdout.
pub enum Output {
    Pager(minus::Pager),
    Direct(io::Stdout),
}

impl Output {
    /// Pick pager or direct stdout. Caller decides which (typically:
    /// pager when stdout is a TTY and `--print` is not set).
    pub fn new(use_pager: bool) -> Self {
        if use_pager {
            Output::Pager(minus::Pager::new())
        } else {
            Output::Direct(io::stdout())
        }
    }

    /// Force a non-paginated direct-stdout output, regardless of TTY or
    /// `--print` flag. Used by `--info`: the output is a fixed-size
    /// summary that doesn't benefit from a pager — to scroll it, use the
    /// interactive viewer's Info mode.
    pub fn direct() -> Self {
        Output::Direct(io::stdout())
    }

    /// Write a line of text to the output.
    pub fn write_line(&mut self, line: &str) -> Result<()> {
        match self {
            Output::Pager(pager) => {
                writeln!(pager, "{line}")?;
            }
            Output::Direct(stdout) => {
                writeln!(stdout, "{line}")?;
            }
        }
        Ok(())
    }

    /// Write raw text (no trailing newline).
    pub fn write_str(&mut self, text: &str) -> Result<()> {
        match self {
            Output::Pager(pager) => {
                write!(pager, "{text}")?;
            }
            Output::Direct(stdout) => {
                write!(stdout, "{text}")?;
            }
        }
        Ok(())
    }

    /// Finalize output. For the pager, this blocks until the user quits.
    pub fn finish(self) -> Result<()> {
        match self {
            Output::Pager(pager) => {
                minus::page_all(pager)?;
            }
            Output::Direct(mut stdout) => {
                stdout.flush()?;
            }
        }
        Ok(())
    }
}
