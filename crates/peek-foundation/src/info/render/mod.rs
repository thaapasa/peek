use syntect::highlighting::Color;

use super::time::format_time;
use super::{FileInfo, InfoValue, Value};
use crate::theme::{PeekTheme, lerp_color};

mod file;

pub use file::format_size_human;

/// Per-render options for the Info view.
#[derive(Clone, Copy, Default)]
pub struct RenderOptions {
    /// When true, show timestamps in UTC (ISO 8601 `...Z`). When false
    /// (default), show local time with `±HH:MM` offset.
    pub utc: bool,
}

pub(super) const LABEL_WIDTH: usize = 14;

/// Render file info as themed terminal lines.
pub fn render(info: &FileInfo, theme: &PeekTheme, opts: RenderOptions) -> Vec<String> {
    let mut lines = Vec::new();

    file::render_section(&mut lines, info, theme, opts.utc);
    info.extras.render_section(&mut lines, theme);

    if !info.warnings.is_empty() {
        lines.push(String::new());
        push_section_header(&mut lines, "Warnings", theme);
        for w in &info.warnings {
            push_field(&mut lines, "Warning", &theme.paint(w, theme.warning), theme);
        }
    }

    lines
}

pub fn push_section_header(lines: &mut Vec<String>, title: &str, theme: &PeekTheme) {
    let rule_len = 40usize.saturating_sub(title.len() + 4);
    let rule = "\u{2500}".repeat(rule_len);
    lines.push(format!(
        "{} {} {}",
        theme.paint_muted("\u{2500}\u{2500}"),
        theme.paint_heading(title),
        theme.paint_muted(&rule),
    ));
}

/// Push a field with a themed label and a pre-colored value.
/// Guarantees at least one space between label and value.
pub fn push_field(lines: &mut Vec<String>, label: &str, colored_value: &str, theme: &PeekTheme) {
    let painted = theme.paint_label(label);
    let pad = if label.len() < LABEL_WIDTH {
        LABEL_WIDTH - label.len()
    } else {
        1
    };
    lines.push(format!("  {}{}{}", painted, " ".repeat(pad), colored_value));
}

/// Paint a count with magnitude-based intensity.
pub fn paint_count(count: usize, theme: &PeekTheme) -> String {
    paint_count_u64(count as u64, theme)
}

/// `u64` form of [`paint_count`] — same gradient, no `usize` round-trip.
/// Used by [`Value::Count`]'s print render.
pub(super) fn paint_count_u64(count: u64, theme: &PeekTheme) -> String {
    let color = count_color(count as usize, theme);
    theme.paint(&thousands_sep(count), color)
}

fn count_color(count: usize, theme: &PeekTheme) -> Color {
    if count == 0 {
        return theme.muted;
    }
    // Logarithmic: 1→0.4, 100→0.6, 10k→0.8, 1M→1.0 of value color
    let magnitude = (count as f64).log10();
    let t = (0.4 + 0.1 * magnitude).clamp(0.4, 1.0) as f32;
    lerp_color(theme.muted, theme.value, t)
}

/// Print half of the semantic [`Value`] enum — the human, themed form that
/// mirrors each variant's machine JSON. The bespoke colouring that used to
/// live inline in `file.rs`'s File section moves here so every section's
/// `Value` fields paint consistently:
/// - `Size`  → `N bytes (H.HH KiB)` on the magnitude gradient
/// - `Count` → thousands-separated, log-intensity colour
/// - `Timestamp` → local time, age-dimmed
/// - `Token`/`Text` → value colour; `Bool` → `yes`/`no`; etc.
impl InfoValue for Value {
    fn render_value(&self, theme: &PeekTheme) -> String {
        match self {
            Value::Size(n) => {
                theme.paint(&file::format_size_display(*n), file::size_color(*n, theme))
            }
            Value::Count(n) => paint_count_u64(*n, theme),
            Value::Int(n) => theme.paint_value(&thousands_sep_signed(*n)),
            Value::Ratio(r) => theme.paint_value(&format!("{r:.2}")),
            Value::Timestamp(t) => {
                // NB: always local time. `InfoValue::render_value` has no
                // `RenderOptions`, so a derived-section timestamp can't honour
                // `--utc` the way the File section's own `paint_timestamp`
                // does. No shipping section constructs a `Value::Timestamp`
                // yet (timestamps arrive pre-formatted from gather), so this is
                // latent — but a future one would silently disagree with the
                // File section under `--utc`. Threading `utc` here needs a
                // `render_value` signature change.
                theme.paint(&format_time(*t, false), file::timestamp_color(*t, theme))
            }
            Value::DurationMs(ms) => theme.paint_value(&format!("{} ms", thousands_sep(*ms))),
            Value::Text(s) | Value::Token(s) => theme.paint_value(s),
            Value::Bool(b) => theme.paint_value(if *b { "yes" } else { "no" }),
            // Pre-formatted: just paint the text in its role; JSON came from the
            // stored `json` (see `Value::Split`).
            Value::Split { text, role, .. } => role.paint(theme, text),
        }
    }
}

/// Thousands-separated signed integer (negatives keep their leading `-`).
fn thousands_sep_signed(n: i64) -> String {
    if n < 0 {
        format!("-{}", thousands_sep(n.unsigned_abs()))
    } else {
        thousands_sep(n as u64)
    }
}

pub fn thousands_sep(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::thousands_sep;
    use crate::info::{InfoValue, Role, Value};
    use crate::theme::{PeekThemeName, StyleMode, load_embedded_theme};

    #[test]
    fn split_paints_text_not_json() {
        let mut theme = crate::theme::PeekTheme::from_syntect(&load_embedded_theme(
            PeekThemeName::default().tmtheme_source(),
        ));
        theme.style_mode = StyleMode::Plain;
        // Print shows `text`; the `json` payload (16) never appears in print.
        let v = Value::split("0x10", Role::Value, serde_json::json!(16));
        assert_eq!(v.render_value(&theme), "0x10");
        assert_eq!(Value::labelled("ELF", "elf").render_value(&theme), "ELF");
    }

    #[test]
    fn thousands_sep_inserts_commas_every_three_digits() {
        assert_eq!(thousands_sep(0), "0");
        assert_eq!(thousands_sep(1), "1");
        assert_eq!(thousands_sep(999), "999");
        assert_eq!(thousands_sep(1_000), "1,000");
        assert_eq!(thousands_sep(12_345), "12,345");
        assert_eq!(thousands_sep(1_234_567), "1,234,567");
    }

    #[test]
    fn thousands_sep_handles_u64_max() {
        assert_eq!(thousands_sep(u64::MAX), "18,446,744,073,709,551,615");
    }
}
