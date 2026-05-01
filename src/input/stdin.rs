use std::io::Read;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::Args;
use crate::input::InputSource;

/// Decide the input sources based on args and stdin state.
///
/// Behavior (the no-args + TTY case is handled in `main.rs`, which shows
/// the help screen before this function is called):
/// - `peek` with stdin piped, no args    → read stdin
/// - `peek -`                            → read stdin (blocks on TTY)
/// - `peek file.rs`                      → file, stdin ignored even if piped
/// - `peek - file.rs`                    → stdin + file
///
/// After consuming piped stdin, fd 0 is reopened from `/dev/tty` so the
/// interactive crossterm event loop can still read keystrokes.
pub fn build_sources(args: &Args) -> Result<Vec<InputSource>> {
    let has_dash = args.files.iter().any(|p| p.as_os_str() == "-");

    // No files + non-TTY stdin → read stdin implicitly.
    let want_stdin = has_dash || args.files.is_empty();

    let stdin_data: Option<Arc<[u8]>> = if want_stdin {
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .context("failed to read stdin")?;
        reopen_stdin_from_tty();
        Some(Arc::from(buf.into_boxed_slice()))
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
                    data: stdin_slot.take().unwrap_or_else(|| Arc::from(Vec::new().into_boxed_slice())),
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
/// device isn't pollable, but the real device node is. If `ttyname` returns
/// nothing for both stderr and stdout, we skip the reopen entirely rather
/// than fall back to `/dev/tty` (which is broken on macOS and unnecessary
/// elsewhere — the resolved path is what works).
///
/// Opened read+write because mio requires writable fds for interest
/// registration, and crossterm uses fd 0 directly when `isatty(0)` is true.
#[cfg(unix)]
fn reopen_stdin_from_tty() {
    use std::os::unix::io::AsRawFd;

    let Some(tty_path) = resolve_tty_path() else {
        return;
    };
    let Ok(tty) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&tty_path)
    else {
        return;
    };
    unsafe {
        libc::dup2(tty.as_raw_fd(), 0);
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
