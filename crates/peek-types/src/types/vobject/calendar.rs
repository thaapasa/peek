//! iCalendar (`.ics`) interpretation: pull events / todos out of the
//! component tree, render them as a readable agenda, and summarise the
//! calendar for the Info sidecar.

use anyhow::Result;

use crate::input::InputSource;
use crate::theme::{PeekTheme, PeekThemeName, StyleMode};
use crate::viewer::modes::{ModeId, TextRenderer};

use super::datetime::{date_key, format_datetime};
use super::line::{Component, ContentLine, format_list, parse_components, unescape_text};
use super::render::{push_field, push_prose};

/// `TextRenderer` for an iCalendar document: a calendar header followed by
/// one block per event / todo. Re-reads + re-parses on each render call;
/// the generic [`crate::viewer::modes::RenderedTextMode`] caches the
/// wrapped output per `(width, style, theme)`.
pub(crate) struct CalendarRenderer {
    source: InputSource,
}

impl CalendarRenderer {
    pub(crate) fn new(source: InputSource) -> Self {
        Self { source }
    }
}

impl TextRenderer for CalendarRenderer {
    fn label(&self) -> &'static str {
        "Calendar"
    }

    fn mode_id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn render(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        _theme_name: PeekThemeName,
        _style_mode: StyleMode,
    ) -> Result<Vec<String>> {
        let width = width.max(20);
        let bytes = self
            .source
            .read_bytes(crate::input::limits::Budget::Sidecar("iCalendar"))?;
        let text = String::from_utf8_lossy(&bytes);
        let roots = parse_components(&text);

        let mut lines = Vec::new();
        let calendars: Vec<&Component> = roots
            .iter()
            .filter(|c| c.name.eq_ignore_ascii_case("VCALENDAR"))
            .collect();
        if calendars.is_empty() {
            return Ok(vec![theme.paint_muted("[no calendar data]")]);
        }

        for cal in calendars {
            render_calendar(&mut lines, cal, theme, width);
        }
        Ok(lines)
    }
}

/// Render one `VCALENDAR`: an optional name header, then every `VEVENT`
/// and `VTODO` in source order.
fn render_calendar(lines: &mut Vec<String>, cal: &Component, theme: &PeekTheme, width: usize) {
    if let Some(name) = cal.value("X-WR-CALNAME") {
        lines.push(theme.paint_heading(&name));
        lines.push(String::new());
    }

    let mut first = true;
    for comp in &cal.children {
        let block = if comp.name.eq_ignore_ascii_case("VEVENT") {
            Some(render_event(comp, theme, width))
        } else if comp.name.eq_ignore_ascii_case("VTODO") {
            Some(render_todo(comp, theme, width))
        } else {
            None
        };
        if let Some(block) = block {
            if !first {
                lines.push(String::new());
            }
            first = false;
            lines.extend(block);
        }
    }

    if first {
        lines.push(theme.paint_muted("[no events]"));
    }
}

/// Render a single `VEVENT` block: summary heading, when, location,
/// recurrence, attendees, status, categories, description.
fn render_event(event: &Component, theme: &PeekTheme, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let summary = event
        .value("SUMMARY")
        .unwrap_or_else(|| "(untitled)".into());
    lines.push(theme.paint_heading(&format!("\u{2022} {summary}")));

    let when = format_when(event.prop("DTSTART"), event.prop("DTEND"));
    push_field(&mut lines, "When", when.as_deref(), theme, width);
    push_field(
        &mut lines,
        "Where",
        event.value("LOCATION").as_deref(),
        theme,
        width,
    );
    if let Some(rrule) = event.prop("RRULE") {
        push_field(
            &mut lines,
            "Repeats",
            Some(&humanize_rrule(&rrule.value)),
            theme,
            width,
        );
    }
    push_attendees(&mut lines, event, theme, width);
    push_field(
        &mut lines,
        "Status",
        event.value("STATUS").map(|s| titlecase(&s)).as_deref(),
        theme,
        width,
    );
    push_field(
        &mut lines,
        "Categories",
        event
            .prop("CATEGORIES")
            .map(|p| format_list(&p.value))
            .as_deref(),
        theme,
        width,
    );
    push_field(
        &mut lines,
        "Link",
        event.value("URL").as_deref(),
        theme,
        width,
    );

    if let Some(desc) = event.value("DESCRIPTION") {
        lines.push(String::new());
        push_prose(&mut lines, &desc, theme, width);
    }
    lines
}

/// Render a single `VTODO` block.
fn render_todo(todo: &Component, theme: &PeekTheme, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let summary = todo.value("SUMMARY").unwrap_or_else(|| "(untitled)".into());
    lines.push(theme.paint_heading(&format!("\u{2611} {summary}")));

    push_field(
        &mut lines,
        "Due",
        todo.prop("DUE")
            .map(|p| format_datetime(&p.value))
            .as_deref(),
        theme,
        width,
    );
    let status = match (todo.value("STATUS"), todo.value("PERCENT-COMPLETE")) {
        (Some(s), Some(pct)) => Some(format!("{} ({pct}%)", titlecase(&s))),
        (Some(s), None) => Some(titlecase(&s)),
        (None, Some(pct)) => Some(format!("{pct}% complete")),
        (None, None) => None,
    };
    push_field(&mut lines, "Status", status.as_deref(), theme, width);
    if let Some(desc) = todo.value("DESCRIPTION") {
        lines.push(String::new());
        push_prose(&mut lines, &desc, theme, width);
    }
    lines
}

/// Emit an `Attendees` row summarising count, plus organizer when present.
fn push_attendees(lines: &mut Vec<String>, event: &Component, theme: &PeekTheme, width: usize) {
    let names: Vec<String> = event.props_named("ATTENDEE").map(attendee_name).collect();
    let organizer = event.prop("ORGANIZER").map(attendee_name);

    let mut parts = Vec::new();
    if let Some(org) = &organizer {
        parts.push(format!("{org} (organizer)"));
    }
    parts.extend(
        names
            .iter()
            .filter(|n| Some(*n) != organizer.as_ref())
            .cloned(),
    );
    if parts.is_empty() {
        return;
    }
    push_field(lines, "Who", Some(&parts.join(", ")), theme, width);
}

/// Display name for an `ATTENDEE` / `ORGANIZER`: the `CN` param if set,
/// else the `mailto:` address stripped of its scheme.
fn attendee_name(prop: &ContentLine) -> String {
    if let Some(cn) = prop.param("CN") {
        return unescape_text(cn.trim_matches('"'));
    }
    prop.value
        .strip_prefix("mailto:")
        .or_else(|| prop.value.strip_prefix("MAILTO:"))
        .unwrap_or(&prop.value)
        .to_string()
}

/// Combine `DTSTART` / `DTEND` into a single readable span. A same-day
/// timed range collapses the redundant date on the end (`… 09:00 – 09:15`).
fn format_when(dtstart: Option<&ContentLine>, dtend: Option<&ContentLine>) -> Option<String> {
    let start = dtstart?;
    let mut out = format_datetime(&start.value);
    if let Some(tz) = start.param("TZID") {
        out.push_str(&format!(" ({tz})"));
    }
    let Some(end) = dtend else {
        return Some(out);
    };
    let end_fmt = format_datetime(&end.value);
    // Collapse the end's date when it repeats the start's (`YYYY-MM-DD `).
    let start_fmt = format_datetime(&start.value);
    let end_tail = match (start_fmt.get(..11), end_fmt.get(..11)) {
        (Some(a), Some(b)) if a == b => end_fmt[11..].to_string(),
        _ => end_fmt,
    };
    out.push_str(&format!(" \u{2013} {end_tail}"));
    Some(out)
}

/// Render an `RRULE` into a short human phrase. Recognises the common
/// `FREQ` / `BYDAY` / `COUNT` / `UNTIL` / `INTERVAL` parts; anything
/// unrecognised is dropped rather than dumped raw, keeping the line short.
fn humanize_rrule(rule: &str) -> String {
    let mut freq = "";
    let mut byday = String::new();
    let mut count = "";
    let mut until = String::new();
    let mut interval = "";
    for part in rule.split(';') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => freq = v,
            "BYDAY" => {
                byday = v
                    .split(',')
                    .map(weekday_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
            "COUNT" => count = v,
            "UNTIL" => until = format_datetime(v),
            "INTERVAL" => interval = v,
            _ => {}
        }
    }

    let base = match freq.to_ascii_uppercase().as_str() {
        "DAILY" => "Daily",
        "WEEKLY" => "Weekly",
        "MONTHLY" => "Monthly",
        "YEARLY" => "Yearly",
        "HOURLY" => "Hourly",
        "" => "Repeats",
        other => return format!("Repeats ({other})"),
    };
    let mut out = base.to_string();
    if !interval.is_empty() && interval != "1" {
        out.push_str(&format!(" (every {interval})"));
    }
    if !byday.is_empty() {
        out.push_str(&format!(" on {byday}"));
    }
    if !count.is_empty() {
        out.push_str(&format!(", {count} times"));
    } else if !until.is_empty() {
        out.push_str(&format!(", until {until}"));
    }
    out
}

/// `MO` → `Mon`, etc. Leading ordinals (`-1SU`) keep the day name.
fn weekday_name(code: &str) -> String {
    let day = code.trim_end_matches(|c: char| !c.is_ascii_alphabetic());
    let tail = &day[day.len().saturating_sub(2)..];
    match tail.to_ascii_uppercase().as_str() {
        "MO" => "Mon",
        "TU" => "Tue",
        "WE" => "Wed",
        "TH" => "Thu",
        "FR" => "Fri",
        "SA" => "Sat",
        "SU" => "Sun",
        _ => code,
    }
    .to_string()
}

/// Lower-case all but the first letter of each word (`CONFIRMED` →
/// `Confirmed`, `NEEDS-ACTION` → `Needs-Action`).
fn titlecase(s: &str) -> String {
    s.split_inclusive(['-', ' '])
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
                }
                None => String::new(),
            }
        })
        .collect()
}

/// Calendar metadata for the Info sidecar.
pub struct CalendarSummary {
    pub name: Option<String>,
    pub version: Option<String>,
    pub product: Option<String>,
    pub event_count: usize,
    pub todo_count: usize,
    /// Earliest / latest event start date (`YYYY-MM-DD`), when any event
    /// carries a `DTSTART`.
    pub date_range: Option<(String, String)>,
}

/// Summarise an iCalendar document: counts, version, product id, and the
/// event date range. Walks the same component tree the renderer uses.
pub fn summarize(text: &str) -> Option<CalendarSummary> {
    let roots = parse_components(text);
    let cal = roots
        .into_iter()
        .find(|c| c.name.eq_ignore_ascii_case("VCALENDAR"))?;

    let mut event_count = 0;
    let mut todo_count = 0;
    let mut min_date: Option<String> = None;
    let mut max_date: Option<String> = None;
    for comp in &cal.children {
        if comp.name.eq_ignore_ascii_case("VEVENT") {
            event_count += 1;
            if let Some(key) = comp.prop("DTSTART").and_then(|p| date_key(&p.value)) {
                if min_date.as_ref().is_none_or(|m| &key < m) {
                    min_date = Some(key.clone());
                }
                if max_date.as_ref().is_none_or(|m| &key > m) {
                    max_date = Some(key);
                }
            }
        } else if comp.name.eq_ignore_ascii_case("VTODO") {
            todo_count += 1;
        }
    }

    Some(CalendarSummary {
        name: cal.value("X-WR-CALNAME"),
        version: cal.value("VERSION"),
        product: cal.value("PRODID"),
        event_count,
        todo_count,
        date_range: min_date.zip(max_date),
    })
}
