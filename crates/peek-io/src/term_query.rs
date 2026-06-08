//! Query the terminal emulator for its background color via the OSC 11
//! escape sequence, so peek can pick a light or dark default theme that
//! matches the surrounding terminal.
//!
//! Unlike cell-size detection (a passive `TIOCGWINSZ` ioctl), this is an
//! active round-trip: we write `ESC ] 11 ; ? BEL` to the terminal and
//! read back `ESC ] 11 ; rgb:rrrr/gggg/bbbb` (BEL- or ST-terminated).
//! That requires putting the tty into raw mode briefly so the reply
//! isn't echoed or line-buffered, then restoring the prior state.
//!
//! Everything here is best-effort and fail-quiet: no controlling
//! terminal, a terminal that ignores the query, or a malformed reply all
//! return `None`, leaving the caller on its built-in default. The read is
//! bounded by a short deadline so a silent terminal can't hang startup.
//!
//! Only meaningful when output goes to a terminal — callers must gate on
//! `is_terminal()` themselves; this module performs no such check.

use std::sync::OnceLock;
#[cfg(unix)]
use std::time::{Duration, Instant};

/// A terminal background color, one byte per channel. Reports wider than
/// 8 bits per channel (the common `rrrr/gggg/bbbb` form) are scaled down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// Perceptual lightness in `0.0..=1.0` (Rec. 601 luma). Used to
    /// classify the background as light vs dark.
    pub fn luma(self) -> f32 {
        let (r, g, b) = (self.r as f32, self.g as f32, self.b as f32);
        (0.299 * r + 0.587 * g + 0.114 * b) / 255.0
    }

    /// Whether this background reads as "light" — luma past the midpoint.
    pub fn is_light(self) -> bool {
        self.luma() >= 0.5
    }
}

/// Cached terminal background — queries once, reuses the result.
///
/// Prime this *before* entering raw mode / the interactive event loop: a
/// live OSC round-trip mid-loop would steal keystroke bytes and write an
/// escape into the rendered frame. Mirrors `cell_size`'s one-shot cache.
pub fn background_color_cached() -> Option<Rgb> {
    static CACHE: OnceLock<Option<Rgb>> = OnceLock::new();
    *CACHE.get_or_init(query_background_color)
}

/// Query the terminal's background color over OSC 11. Returns `None` when
/// there's no terminal, the terminal doesn't answer within the deadline,
/// or the reply can't be parsed. See the module docs for the protocol.
#[cfg(unix)]
pub fn query_background_color() -> Option<Rgb> {
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::unix::io::AsRawFd;

    // Prefer the device path the rest of the codebase resolves; fall back
    // to /dev/tty for the plain interactive case where it's always valid.
    let tty_path = crate::stdin::resolve_tty_path().unwrap_or_else(|| "/dev/tty".to_string());
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&tty_path)
        .ok()?;
    let fd = tty.as_raw_fd();

    // Raw mode so the reply isn't echoed or held by line buffering. The
    // guard restores the saved settings on every exit path.
    let _raw = RawMode::enable(fd)?;

    // OSC 11 background-color query, BEL-terminated (the widely supported
    // form; we also accept an ST-terminated reply when reading).
    tty.write_all(b"\x1b]11;?\x07").ok()?;
    tty.flush().ok()?;

    let mut buf = Vec::with_capacity(64);
    let mut chunk = [0u8; 64];
    let deadline = Instant::now() + Duration::from_millis(120);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || !poll_readable(fd, remaining) {
            break;
        }
        match tty.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if reply_complete(&buf) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    parse_osc11(&buf)
}

#[cfg(not(unix))]
pub fn query_background_color() -> Option<Rgb> {
    // OSC 11 round-trips aren't reliable across Windows consoles (only
    // newer Windows Terminal answers, and the conhost path doesn't), so
    // stay conservative and let the caller keep its default theme.
    None
}

/// True once the buffer holds a terminator for the OSC reply — BEL or the
/// ST sequence `ESC \`.
#[cfg(unix)]
fn reply_complete(buf: &[u8]) -> bool {
    buf.contains(&0x07) || buf.windows(2).any(|w| w == [0x1b, b'\\'])
}

/// Parse `…rgb:rrrr/gggg/bbbb…` out of an OSC 11 reply. Each channel may
/// carry 1–4 hex digits; values are scaled to 8 bits by digit width.
#[cfg(unix)]
fn parse_osc11(buf: &[u8]) -> Option<Rgb> {
    let text = std::str::from_utf8(buf).ok()?;
    let rest = &text[text.find("rgb:")? + 4..];
    // Channels run until the terminator (BEL / ESC) or end of string.
    let end = rest.find(['\x07', '\x1b']).unwrap_or(rest.len());
    let mut parts = rest[..end].split('/');
    let r = scale_channel(parts.next()?)?;
    let g = scale_channel(parts.next()?)?;
    let b = scale_channel(parts.next()?)?;
    Some(Rgb { r, g, b })
}

/// Scale a hex channel of arbitrary width (1–4 digits) to a single byte.
#[cfg(unix)]
fn scale_channel(hex: &str) -> Option<u8> {
    let hex = hex.trim();
    if hex.is_empty() || hex.len() > 4 {
        return None;
    }
    let val = u32::from_str_radix(hex, 16).ok()?;
    let max = (1u32 << (4 * hex.len())) - 1;
    Some(((val * 255 + max / 2) / max) as u8)
}

/// Wait up to `timeout` for `fd` to become readable. False on timeout or
/// poll error.
#[cfg(unix)]
fn poll_readable(fd: i32, timeout: Duration) -> bool {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    let rc = unsafe { libc::poll(&mut pfd, 1, ms) };
    rc > 0 && (pfd.revents & libc::POLLIN) != 0
}

/// RAII raw-mode guard: flips the tty into a minimal raw mode (no canon,
/// no echo) on construction and restores the prior `termios` on drop.
#[cfg(unix)]
struct RawMode {
    fd: i32,
    saved: libc::termios,
}

#[cfg(unix)]
impl RawMode {
    fn enable(fd: i32) -> Option<Self> {
        unsafe {
            let mut saved: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut saved) != 0 {
                return None;
            }
            let mut raw = saved;
            raw.c_lflag &= !(libc::ICANON | libc::ECHO);
            // Non-blocking reads; the deadline + poll() drive timing.
            raw.c_cc[libc::VMIN] = 0;
            raw.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
                return None;
            }
            Some(RawMode { fd, saved })
        }
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.saved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_16bit_reply() {
        let reply = b"\x1b]11;rgb:ffff/ffff/ffff\x07";
        assert_eq!(
            parse_osc11(reply),
            Some(Rgb {
                r: 255,
                g: 255,
                b: 255
            })
        );
    }

    #[test]
    fn parses_st_terminated_reply() {
        let reply = b"\x1b]11;rgb:0000/0000/0000\x1b\\";
        assert_eq!(parse_osc11(reply), Some(Rgb { r: 0, g: 0, b: 0 }));
    }

    #[test]
    fn parses_8bit_channels() {
        let reply = b"\x1b]11;rgb:1e/1e/1e\x07";
        assert_eq!(
            parse_osc11(reply),
            Some(Rgb {
                r: 0x1e,
                g: 0x1e,
                b: 0x1e
            })
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_osc11(b"no color here"), None);
    }

    #[test]
    fn light_dark_classification() {
        assert!(
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
            .is_light()
        );
        assert!(
            !Rgb {
                r: 30,
                g: 30,
                b: 30
            }
            .is_light()
        );
    }
}
