use std::fmt::Write as FmtWrite;
use std::io::{self, IsTerminal, Write};

use anyhow::Result;

use crate::Args;

/// Output abstraction: pager, direct stdout, or in-memory buffer.
#[allow(dead_code)]
pub enum Output {
    Pager(minus::Pager),
    Direct(io::Stdout),
    Buffer(Vec<String>),
}

impl Output {
    pub fn new(args: &Args) -> Result<Self> {
        let use_pager = !args.print && io::stdout().is_terminal();

        if use_pager {
            let pager = minus::Pager::new();
            Ok(Output::Pager(pager))
        } else {
            Ok(Output::Direct(io::stdout()))
        }
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
            Output::Buffer(lines) => {
                lines.push(line.to_string());
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
            Output::Buffer(lines) => {
                // Append to the last line or start a new one
                if let Some(last) = lines.last_mut() {
                    last.push_str(text);
                } else {
                    lines.push(text.to_string());
                }
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
            Output::Buffer(_) => {}
        }
        Ok(())
    }
}
