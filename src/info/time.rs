//! Timestamp formatting for FileInfo.
//!
//! Two output forms:
//!
//! - **UTC ISO 8601** (`2025-01-15T14:30:00Z`) — used when `--utc` is set,
//!   and as the fallback whenever local time can't be resolved.
//! - **Local time with offset** (`2025-01-15 09:30:00 -05:00`) — the
//!   default on Unix.
//!
//! ### Why this is correct without `chrono`
//!
//! The hard part of timestamp formatting is "what offset applies for
//! this Unix epoch in this zone?" — DST transitions, historical
//! offset changes, political-zone redrawings, etc. That answer lives
//! in the OS's tzdata (`/etc/localtime`, `/usr/share/zoneinfo`), and
//! the only way to read it is via `localtime_r` / equivalent. `chrono`
//! ends up calling `localtime_r` under the hood on Unix too — adding
//! it would just bundle a second copy of the tzdata reader without
//! making the answer more correct.
//!
//! UTC, by contrast, has no zone rules: a fixed zero offset and a
//! proleptic Gregorian calendar. `format_iso_utc` does the date math
//! directly using Howard Hinnant's "days from civil" algorithm, which
//! folds every leap-year / century / 400-year exception into integer
//! arithmetic — exact for any date by construction.
//!
//! The one thing this approach gives up is local-time formatting on
//! Windows, which has no `localtime_r` analogue exposing `tm_gmtoff`
//! cleanly through `libc`. We fall back to UTC there.

use std::time::SystemTime;

/// Format a `SystemTime` as either UTC ISO-8601 or local time with offset.
///
/// On non-Unix platforms (and on Unix if `localtime_r` fails), the local
/// path falls back to UTC.
pub(super) fn format_time(time: SystemTime, utc: bool) -> String {
    let duration = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => return "unknown".to_string(),
    };
    let secs = duration.as_secs() as i64;

    if utc {
        return format_iso_utc(secs);
    }
    #[cfg(unix)]
    {
        if let Some(s) = format_local_with_offset(secs) {
            return s;
        }
    }
    format_iso_utc(secs)
}

/// Compact UTC stamp `YYYY-MM-DD HH:MM` for column-aligned listings
/// (archive TOC, etc.). Returns a fixed-width 16-char string; on overflow
/// or formatting failure, returns a 16-char `-` filler so column widths
/// don't shift.
pub fn format_archive_mtime(secs: u64) -> String {
    let iso = format_iso_utc(secs as i64);
    if iso.len() >= 16 {
        format!("{} {}", &iso[..10], &iso[11..16])
    } else {
        format!("{:<16}", "-")
    }
}

/// Compact archive mtime with timezone marker. UTC-mode appends ` Z`;
/// local-mode appends a short offset (`+3`, `-5:30`, `+0`). Falls back
/// to UTC when `localtime_r` fails (extreme date or broken tzdata).
pub fn format_archive_mtime_zoned(secs: u64, utc: bool) -> String {
    if utc {
        return format!("{} Z", format_archive_mtime(secs));
    }
    #[cfg(unix)]
    {
        if let Some(s) = format_local_short(secs as i64) {
            return s;
        }
    }
    format!("{} Z", format_archive_mtime(secs))
}

#[cfg(unix)]
fn format_local_short(secs: i64) -> Option<String> {
    // SAFETY: localtime_r writes a caller-provided struct; null return =
    // failure (out-of-range date or tzdata problem). Same pattern as
    // `format_local_with_offset`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t: libc::time_t = secs;
    let result = unsafe { libc::localtime_r(&t, &mut tm) };
    if result.is_null() {
        return None;
    }
    let year = tm.tm_year as i64 + 1900;
    let month = tm.tm_mon + 1;
    let day = tm.tm_mday;
    let hours = tm.tm_hour;
    let minutes = tm.tm_min;
    let offset = format_offset_short(tm.tm_gmtoff);
    Some(format!(
        "{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02} {offset}"
    ))
}

/// Short timezone offset: `+3`, `-5`, `+0` for whole-hour zones; the
/// minute component appears only when nonzero (`+5:30`).
fn format_offset_short(secs: i64) -> String {
    let sign = if secs >= 0 { '+' } else { '-' };
    let abs = secs.unsigned_abs();
    let h = abs / 3600;
    let m = (abs % 3600) / 60;
    if m == 0 {
        format!("{sign}{h}")
    } else {
        format!("{sign}{h}:{m:02}")
    }
}

/// Format `secs` (Unix epoch seconds) as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Pure proleptic-Gregorian arithmetic — no tz data needed, since UTC
/// has no DST and a fixed zero offset. Negative timestamps (pre-1970)
/// clamp to the epoch, which is fine for filesystem metadata.
fn format_iso_utc(secs: i64) -> String {
    let s = secs.max(0) as u64;
    let days = s / 86400;
    let tod = s % 86400;
    let (year, month, day) = days_to_date(days);
    let hours = tod / 3600;
    let minutes = (tod % 3600) / 60;
    let seconds = tod % 60;
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Format `secs` as local time with a `±HH:MM` offset using the OS tz
/// database.
///
/// `libc::localtime_r` reads `/etc/localtime` (and honors the `TZ` env
/// var) to apply DST, historical offset transitions, and political
/// zone changes. Returns `None` only if the underlying call fails
/// (extreme date or broken tzdata); the caller falls back to UTC.
#[cfg(unix)]
fn format_local_with_offset(secs: i64) -> Option<String> {
    // SAFETY: localtime_r writes to a caller-provided struct; we pass a
    // zero-initialized one and check the return for null.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t: libc::time_t = secs;
    let result = unsafe { libc::localtime_r(&t, &mut tm) };
    if result.is_null() {
        return None;
    }
    let year = tm.tm_year as i64 + 1900;
    let month = tm.tm_mon + 1;
    let day = tm.tm_mday;
    let hours = tm.tm_hour;
    let minutes = tm.tm_min;
    let seconds = tm.tm_sec;
    let off = tm.tm_gmtoff;
    let sign = if off >= 0 { '+' } else { '-' };
    let off_abs = off.unsigned_abs();
    let off_h = off_abs / 3600;
    let off_m = (off_abs % 3600) / 60;
    Some(format!(
        "{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02} {sign}{off_h:02}:{off_m:02}"
    ))
}

/// Convert days since the Unix epoch (1970-01-01) to `(year, month, day)`.
///
/// Howard Hinnant's "days from civil" algorithm
/// (<http://howardhinnant.github.io/date_algorithms.html>) — proleptic
/// Gregorian, exact for any date. The leap-year, century, and 400-year
/// exceptions are encoded directly in the integer math, so there are no
/// table lookups or special cases to get wrong.
fn days_to_date(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_utc_at_epoch() {
        assert_eq!(format_iso_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn iso_utc_known_date() {
        // 2025-01-15 14:30:00 UTC
        assert_eq!(format_iso_utc(1_736_951_400), "2025-01-15T14:30:00Z");
    }

    #[test]
    fn iso_utc_leap_day_2024() {
        // 2024-02-29 12:00:00 UTC — ordinary leap year (divisible by 4).
        assert_eq!(format_iso_utc(1_709_208_000), "2024-02-29T12:00:00Z");
    }

    #[test]
    fn iso_utc_400_year_leap_2000() {
        // 2000-02-29 00:00:00 UTC — leap year by the 400-year rule
        // (divisible by 100 AND by 400). The case most likely to be
        // miscoded by hand-rolled calendars.
        assert_eq!(format_iso_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn iso_utc_end_of_year() {
        // 2023-12-31 23:59:59 UTC — last second of a year.
        assert_eq!(format_iso_utc(1_704_067_199), "2023-12-31T23:59:59Z");
    }

    #[test]
    fn iso_utc_negative_clamps_to_epoch() {
        // Negative timestamps (pre-1970) clamp; acceptable for filesystem
        // metadata where pre-epoch mtimes don't occur in practice.
        assert_eq!(format_iso_utc(-1), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn format_time_utc_roundtrip() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_736_951_400);
        assert_eq!(format_time(t, true), "2025-01-15T14:30:00Z");
    }

    #[cfg(unix)]
    #[test]
    fn local_offset_smoke() {
        // The exact wall-clock and offset depend on the test machine's tz,
        // so verify only the structural shape: YYYY-MM-DD HH:MM:SS ±HH:MM
        let s = format_local_with_offset(1_736_951_400).expect("localtime_r");
        assert_eq!(s.len(), "2025-01-15 14:30:00 +00:00".len());
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], " ");
        assert_eq!(&s[13..14], ":");
        assert_eq!(&s[16..17], ":");
        assert_eq!(&s[19..20], " ");
        let sign = &s[20..21];
        assert!(sign == "+" || sign == "-", "expected ± offset, got {s:?}");
    }
}
