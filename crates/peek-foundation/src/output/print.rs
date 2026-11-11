//! Print-mode output.
//!
//! Write-once, non-interactive stdout sink used for `--print`, piped runs
//! (stdout is not a TTY), and `--info`. The primary mode's
//! `render_to_pipe` writes its whole rendering through a `PrintOutput`
//! and returns; there's no event loop here.
//!
//! For the interactive TTY path (alternate screen, mode stack, key
//! dispatch, animation ticks), see `the interactive event loop`.
//!
//! Also owns the "reader closed stdout" tag ([`StdoutClosed`]). Every
//! stdout write site maps its errors through [`stdout_error`], so `main`
//! can end `peek … | head` quietly via [`is_stdout_closed`]. The tag is
//! applied only at stdout writes: an EPIPE from a child-process pipe
//! (Ghostscript, `sh` in `--update`) stays a real error. peek keeps Rust's
//! ignored SIGPIPE rather than restoring the default, which would kill the
//! process without running Drop (spooled temp files, terminal restore).

use std::fmt;
use std::io::{self, Write};

use anyhow::Result;

/// The reader of stdout went away (`EPIPE`). Not a failure: the user
/// stopped reading (`| head`), so `main` exits 0 without a message.
#[derive(Debug)]
pub struct StdoutClosed(io::Error);

impl fmt::Display for StdoutClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stdout closed: {}", self.0)
    }
}

impl std::error::Error for StdoutClosed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Convert an error from a stdout write, tagging a broken pipe as
/// [`StdoutClosed`].
pub fn stdout_error(e: io::Error) -> anyhow::Error {
    if e.kind() == io::ErrorKind::BrokenPipe {
        StdoutClosed(e).into()
    } else {
        e.into()
    }
}

/// Whether `e` (at any context depth) is a tagged closed stdout.
pub fn is_stdout_closed(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.is::<StdoutClosed>())
}

/// Write a prepared buffer to stdout and flush, tagging a closed pipe.
/// For one-shot screens (help, version, completions) built in memory.
pub fn write_stdout(bytes: &[u8]) -> Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(bytes).map_err(stdout_error)?;
    out.flush().map_err(stdout_error)
}

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
        writeln!(self.stdout, "{line}").map_err(stdout_error)
    }

    /// Write raw text (no trailing newline).
    pub fn write_str(&mut self, text: &str) -> Result<()> {
        write!(self.stdout, "{text}").map_err(stdout_error)
    }

    /// Finalize output: flush stdout.
    pub fn finish(mut self) -> Result<()> {
        self.stdout.flush().map_err(stdout_error)
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Context;

    use super::*;

    #[test]
    fn broken_pipe_is_tagged_through_context() {
        let e = Err::<(), _>(stdout_error(io::Error::from(io::ErrorKind::BrokenPipe)))
            .context("failed to render x")
            .unwrap_err();
        assert!(is_stdout_closed(&e));
    }

    #[test]
    fn other_errors_and_untagged_broken_pipes_are_not() {
        let other = stdout_error(io::Error::from(io::ErrorKind::PermissionDenied));
        assert!(!is_stdout_closed(&other));
        // A child-process pipe (Ghostscript, `sh`) never goes through the tag.
        let child = anyhow::Error::from(io::Error::from(io::ErrorKind::BrokenPipe));
        assert!(!is_stdout_closed(&child));
    }
}
