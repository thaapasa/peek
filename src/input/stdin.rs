use std::io::Read;

use anyhow::{Context, Result};
use bytes::Bytes;

use crate::Args;
use crate::input::InputSource;

/// Decide the input source based on args and stdin state.
///
/// peek is a single-file viewer; the no-args + TTY case is handled in
/// `main.rs` (shows the help screen before this is called).
///
/// - `peek` with stdin piped, no args → read stdin
/// - `peek -`                         → read stdin (blocks on TTY)
/// - `peek file.rs`                   → file, stdin ignored even if piped
///
/// After consuming piped stdin, fd 0 is reopened from `/dev/tty` so the
/// interactive crossterm event loop can still read keystrokes.
pub fn build_source(args: &Args) -> Result<InputSource> {
    let is_dash = args.file.as_ref().is_some_and(|p| p.as_os_str() == "-");
    let want_stdin = is_dash || args.file.is_none();

    if want_stdin {
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .context("failed to read stdin")?;
        reopen_stdin_from_tty();
        return Ok(InputSource::stdin(Bytes::from(buf)));
    }

    Ok(InputSource::File(args.file.clone().expect("file present")))
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

/// Windows counterpart: reopen the console input buffer through the
/// reserved device name `CONIN$` and route `STD_INPUT_HANDLE` at it.
/// crossterm on Windows reads via `GetStdHandle(STD_INPUT_HANDLE)`, so
/// `SetStdHandle` is the handle the event loop will pick up. The
/// C-runtime fd 0 stays pointed at the consumed pipe — peek doesn't
/// read stdin through libc, so that's fine.
///
/// No-op when the process has no attached console (GUI launch, daemon-
/// like contexts): `CreateFileW("CONIN$", …)` returns
/// `INVALID_HANDLE_VALUE` and we silently bail, matching the Unix
/// `ttyname()`-returns-nothing path.
///
/// All Win32 console FFI lives in this function — nothing else in the
/// codebase touches `windows-sys` or console handles.
#[cfg(windows)]
fn reopen_stdin_from_tty() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{STD_INPUT_HANDLE, SetStdHandle};

    // UTF-16, NUL-terminated — `CreateFileW` takes a wide string.
    let name: Vec<u16> = std::ffi::OsStr::new("CONIN$")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: `name` is a valid NUL-terminated UTF-16 buffer for the
    // lifetime of the call. The other pointer args are explicitly null
    // (no security attributes / no template file), which CreateFileW
    // documents as valid.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return;
    }

    // SAFETY: `handle` is a freshly-opened, owned console handle.
    // Ownership transfers to the standard-handle table; we intentionally
    // do not close it here — `SetStdHandle` retains the handle for the
    // process's lifetime.
    unsafe {
        SetStdHandle(STD_INPUT_HANDLE, handle);
    }
}

#[cfg(all(not(unix), not(windows)))]
fn reopen_stdin_from_tty() {
    // Other targets (wasm, etc.) — peek is a CLI binary so this arm is
    // effectively unreachable, but keeping a no-op fallback means the
    // module still builds out-of-tree.
}
