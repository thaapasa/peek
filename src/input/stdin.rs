use std::io::{IsTerminal, Read};

use anyhow::{Context, Result, bail};

use crate::Args;
use crate::input::InputSource;

/// Decide the input sources based on args and stdin state.
///
/// Behavior:
/// - `peek` with stdin TTY and no args   → error
/// - `peek` with stdin piped, no args    → read stdin
/// - `peek -`                            → read stdin (blocks on TTY)
/// - `peek file.rs`                      → file, stdin ignored even if piped
/// - `peek - file.rs`                    → stdin + file
///
/// After consuming piped stdin, fd 0 is reopened from `/dev/tty` so the
/// interactive crossterm event loop can still read keystrokes.
pub fn build_sources(args: &Args) -> Result<Vec<InputSource>> {
    let has_dash = args.files.iter().any(|p| p.as_os_str() == "-");
    let stdin_is_tty = std::io::stdin().is_terminal();
    let implicit_stdin = args.files.is_empty() && !stdin_is_tty;

    if args.files.is_empty() && !implicit_stdin {
        bail!("no files specified; run `peek --help` for usage");
    }

    let want_stdin = has_dash || implicit_stdin;

    let stdin_data = if want_stdin {
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .context("failed to read stdin")?;
        reopen_stdin_from_tty();
        Some(buf)
    } else {
        None
    };

    if args.files.is_empty() {
        return Ok(vec![InputSource::Stdin {
            data: stdin_data.expect("stdin requested but not read"),
        }]);
    }

    let mut stdin_slot = stdin_data;
    let sources = args
        .files
        .iter()
        .map(|p| {
            if p.as_os_str() == "-" {
                // First `-` takes the data; extra `-`s get an empty buffer.
                InputSource::Stdin {
                    data: stdin_slot.take().unwrap_or_default(),
                }
            } else {
                InputSource::File(p.clone())
            }
        })
        .collect();

    Ok(sources)
}

/// Reopen fd 0 from the controlling terminal after piped stdin is consumed,
/// so crossterm's event loop can read keystrokes. No-op if no TTY is available.
///
/// Uses `ttyname()` on stderr/stdout to resolve the actual device path (e.g.
/// `/dev/ttys000`) rather than opening `/dev/tty`. On macOS, kqueue rejects
/// `/dev/tty` with EINVAL when mio tries to register it — the magic routing
/// device isn't pollable, but the real device node is.
///
/// Opened read+write because mio requires writable fds for interest
/// registration, and crossterm uses fd 0 directly when `isatty(0)` is true.
#[cfg(unix)]
fn reopen_stdin_from_tty() {
    use std::os::unix::io::AsRawFd;

    let tty_path = resolve_tty_path();
    let open_result = tty_path
        .as_deref()
        .or(Some("/dev/tty"))
        .and_then(|p| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(p)
                .ok()
        });
    if let Some(tty) = open_result {
        unsafe {
            libc::dup2(tty.as_raw_fd(), 0);
        }
    }
}

/// Resolve the controlling terminal's device path by calling `ttyname()` on
/// stderr, then stdout. Returns `None` if neither is a TTY.
#[cfg(unix)]
fn resolve_tty_path() -> Option<String> {
    for fd in [2, 1] {
        unsafe {
            let p = libc::ttyname(fd);
            if !p.is_null() {
                return Some(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned());
            }
        }
    }
    None
}

#[cfg(not(unix))]
fn reopen_stdin_from_tty() {
    // TODO: Windows support via CONIN$
}
