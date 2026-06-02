//! Date / date-time formatting for vObject property values.
//!
//! iCalendar and vCard both serialise dates in ISO 8601 basic form —
//! `20260112T090000`, a UTC `…Z` suffix, or a date-only `20260106` — and
//! vCard v4 also permits the extended `1988-04-12` form. We reformat to a
//! readable `YYYY-MM-DD HH:MM` without pulling in a date crate: the values
//! are already calendar fields, so it's pure string reshaping, not time
//! math (no zone conversion — the original offset/zone is shown verbatim).

/// Format an iCalendar `DATE` / `DATE-TIME` value for display. Falls back
/// to the raw string when it doesn't match a known shape, so unusual
/// values still surface rather than vanishing.
pub fn format_datetime(value: &str) -> String {
    let v = value.trim();
    let (body, utc) = match v.strip_suffix('Z') {
        Some(rest) => (rest, true),
        None => (v, false),
    };

    let formatted = match body.split_once('T') {
        Some((date, time)) => match (format_date(date), format_time(time)) {
            (Some(d), Some(t)) => format!("{d} {t}"),
            _ => return value.to_string(),
        },
        None => match format_date(body) {
            Some(d) => d,
            None => return value.to_string(),
        },
    };

    if utc {
        format!("{formatted} UTC")
    } else {
        formatted
    }
}

/// `20260112` or `2026-01-12` → `2026-01-12`.
fn format_date(date: &str) -> Option<String> {
    let digits: String = date.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 8 {
        return None;
    }
    Some(format!(
        "{}-{}-{}",
        &digits[0..4],
        &digits[4..6],
        &digits[6..8]
    ))
}

/// `090000` or `0900` → `09:00`. Seconds are dropped (minute precision is
/// enough for a scannable list).
fn format_time(time: &str) -> Option<String> {
    let digits: String = time.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return None;
    }
    Some(format!("{}:{}", &digits[0..2], &digits[2..4]))
}

/// Sortable `YYYY-MM-DD` key for a date(-time) value, or `None` when it
/// carries no recognisable date. Fixed-width, so lexicographic ordering
/// matches chronological ordering — used to compute a calendar's date
/// range without parsing into real dates.
pub fn date_key(value: &str) -> Option<String> {
    let body = value.trim().trim_end_matches('Z');
    let date = body.split('T').next().unwrap_or(body);
    format_date(date)
}
