//! The Content/Source info section for text-based files, driven by a single
//! [`TextView`] that derives *both* `serde::Serialize` (the `--info --json`
//! form) and [`InfoSection`](crate::info::InfoSection) (the themed print
//! form). Labels, skip rules, and per-field formatting are declared once on
//! the view; the two outputs fall out of the derives.
//!
//! [`TextStats`] stays the streaming-gather accumulator (and the struct
//! `types::svg` embeds); `TextView` is the presentation projection built from
//! it. The enums carry their own split formatting — `Serialize` emits the
//! machine token, [`InfoValue`](crate::info::InfoValue) the human label.

use serde::{Serialize, Serializer};

use crate::info::{InfoSection, InfoValue, Value, push_field, render_info_section};
use crate::theme::PeekTheme;
use crate::types::text::info::{Encoding, IndentStyle, LineEndings, TextStats};

/// Themed terminal Content section for plain text files.
pub fn render_section(lines: &mut Vec<String>, stats: &TextStats, theme: &PeekTheme) {
    render_info_section(lines, &TextView::from(stats), theme);
}

/// Push the text-stat rows without a section header — used by `types::svg`,
/// which folds them under its own "Source" header.
pub fn push_text_stats(lines: &mut Vec<String>, stats: &TextStats, theme: &PeekTheme) {
    for (label, value) in TextView::from(stats).rows(theme) {
        push_field(lines, label, &value, theme);
    }
}

/// Typed `--info --json` view of the Content section, nested under `"text"`.
pub fn json_section(stats: &TextStats) -> (&'static str, serde_json::Value) {
    (
        "text",
        serde_json::to_value(TextView::from(stats)).expect("text info view serializes"),
    )
}

/// One struct, two outputs. Field order is the print order; JSON key order is
/// serde_json's (alphabetical), so the two never need to agree on order.
#[derive(Serialize, InfoSection)]
#[info(title = "Content")]
struct TextView {
    #[info(label = "Lines")]
    line_count: Value,
    // JSON keeps `0`; print hides a zero blank-line count.
    #[info(label = "Blank Lines", skip_if_zero)]
    blank_lines: Value,
    #[info(label = "Words")]
    word_count: Value,
    #[info(label = "Characters")]
    char_count: Value,
    #[info(label = "Longest Line", skip_if_zero)]
    longest_line_chars: Value,
    #[info(label = "Line Endings")]
    line_endings: LineEndings,
    #[info(label = "Indent")]
    #[serde(skip_serializing_if = "Option::is_none")]
    indent: Option<IndentStyle>,
    #[info(label = "Encoding")]
    encoding: Encoding,
    #[info(label = "Shebang")]
    #[serde(skip_serializing_if = "Option::is_none")]
    shebang: Option<String>,
}

impl From<&TextStats> for TextView {
    fn from(s: &TextStats) -> Self {
        TextView {
            line_count: Value::count(s.line_count as u64),
            blank_lines: Value::count(s.blank_lines as u64),
            word_count: Value::count(s.word_count as u64),
            char_count: Value::count(s.char_count as u64),
            longest_line_chars: Value::count(s.longest_line_chars as u64),
            line_endings: s.line_endings,
            indent: s.indent_style,
            encoding: s.encoding,
            shebang: s.shebang.clone(),
        }
    }
}

// --- per-field split formatting -------------------------------------------

impl InfoValue for LineEndings {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_value(line_endings_label(*self))
    }
}

impl Serialize for LineEndings {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(line_endings_token(*self))
    }
}

impl InfoValue for Encoding {
    fn render_value(&self, theme: &PeekTheme) -> String {
        // Encoding renders muted: it's rarely the interesting field.
        theme.paint_muted(encoding_label(*self))
    }
}

impl Serialize for Encoding {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(encoding_token(*self))
    }
}

impl InfoValue for IndentStyle {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_value(&indent_label(*self))
    }
}

impl Serialize for IndentStyle {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        match self {
            IndentStyle::Spaces(n) => {
                let mut st = ser.serialize_struct("indent", 2)?;
                st.serialize_field("style", "spaces")?;
                st.serialize_field("width", n)?;
                st.end()
            }
            other => {
                let mut st = ser.serialize_struct("indent", 1)?;
                st.serialize_field(
                    "style",
                    if matches!(other, IndentStyle::Tabs) {
                        "tabs"
                    } else {
                        "mixed"
                    },
                )?;
                st.end()
            }
        }
    }
}

fn line_endings_label(le: LineEndings) -> &'static str {
    match le {
        LineEndings::None => "none",
        LineEndings::Lf => "LF (\\n)",
        LineEndings::Crlf => "CRLF (\\r\\n)",
        LineEndings::Cr => "CR (\\r)",
        LineEndings::Mixed => "mixed",
    }
}

fn line_endings_token(le: LineEndings) -> &'static str {
    match le {
        LineEndings::None => "none",
        LineEndings::Lf => "lf",
        LineEndings::Crlf => "crlf",
        LineEndings::Cr => "cr",
        LineEndings::Mixed => "mixed",
    }
}

fn indent_label(style: IndentStyle) -> String {
    match style {
        IndentStyle::Tabs => "tabs".to_string(),
        IndentStyle::Spaces(n) => format!("{n} spaces"),
        IndentStyle::Mixed => "mixed".to_string(),
    }
}

fn encoding_label(enc: Encoding) -> &'static str {
    match enc {
        Encoding::Utf8 => "UTF-8",
        Encoding::Utf8Bom => "UTF-8 (BOM)",
        Encoding::Utf16Le => "UTF-16 LE",
        Encoding::Utf16Be => "UTF-16 BE",
    }
}

fn encoding_token(enc: Encoding) -> &'static str {
    match enc {
        Encoding::Utf8 => "utf-8",
        Encoding::Utf8Bom => "utf-8-bom",
        Encoding::Utf16Le => "utf-16-le",
        Encoding::Utf16Be => "utf-16-be",
    }
}

#[cfg(test)]
mod print_tests {
    use super::*;
    use crate::theme::{PeekTheme, PeekThemeName, StyleMode, load_embedded_theme};

    fn plain_theme() -> PeekTheme {
        let mut t = PeekTheme::from_syntect(&load_embedded_theme(
            PeekThemeName::default().tmtheme_source(),
        ));
        t.style_mode = StyleMode::Plain;
        t
    }

    /// The derived `InfoSection` rows: declaration order, zero counts and
    /// `None` optionals skipped, enum fields rendered as their human label.
    #[test]
    fn rows_skip_zero_and_none_and_use_human_labels() {
        let stats = TextStats {
            line_count: 10,
            word_count: 20,
            char_count: 100,
            blank_lines: 0,        // skip_if_zero → hidden
            longest_line_chars: 0, // skip_if_zero → hidden
            line_endings: LineEndings::Crlf,
            indent_style: None, // None → hidden
            encoding: Encoding::Utf8Bom,
            shebang: None, // None → hidden
        };
        let rows = TextView::from(&stats).rows(&plain_theme());
        let labels: Vec<&str> = rows.iter().map(|(l, _)| *l).collect();
        assert_eq!(
            labels,
            ["Lines", "Words", "Characters", "Line Endings", "Encoding"]
        );
        let by = |name: &str| rows.iter().find(|(l, _)| *l == name).unwrap().1.clone();
        assert_eq!(by("Lines"), "10");
        // Human label, not the JSON token.
        assert_eq!(by("Line Endings"), "CRLF (\\r\\n)");
        assert_eq!(by("Encoding"), "UTF-8 (BOM)");
    }

    #[test]
    fn present_optionals_render() {
        let stats = TextStats {
            line_count: 3,
            word_count: 6,
            char_count: 30,
            blank_lines: 1,
            longest_line_chars: 12,
            line_endings: LineEndings::Lf,
            indent_style: Some(IndentStyle::Spaces(4)),
            encoding: Encoding::Utf8,
            shebang: Some("#!/bin/sh".to_string()),
        };
        let rows = TextView::from(&stats).rows(&plain_theme());
        let labels: Vec<&str> = rows.iter().map(|(l, _)| *l).collect();
        assert_eq!(
            labels,
            [
                "Lines",
                "Blank Lines",
                "Words",
                "Characters",
                "Longest Line",
                "Line Endings",
                "Indent",
                "Encoding",
                "Shebang"
            ]
        );
        let by = |name: &str| rows.iter().find(|(l, _)| *l == name).unwrap().1.clone();
        assert_eq!(by("Indent"), "4 spaces");
        assert_eq!(by("Shebang"), "#!/bin/sh");
    }
}

#[cfg(test)]
mod json_tests {
    use super::*;

    #[test]
    fn machine_tokens_and_omitted_optionals() {
        let stats = TextStats {
            line_count: 10,
            word_count: 20,
            char_count: 100,
            blank_lines: 2,
            longest_line_chars: 40,
            line_endings: LineEndings::Crlf,
            indent_style: Some(IndentStyle::Spaces(4)),
            encoding: Encoding::Utf8Bom,
            shebang: None,
        };
        let (key, v) = json_section(&stats);
        assert_eq!(key, "text");
        assert_eq!(v["line_count"], serde_json::json!(10));
        assert_eq!(v["line_endings"], serde_json::json!("crlf"));
        assert_eq!(v["encoding"], serde_json::json!("utf-8-bom"));
        assert_eq!(
            v["indent"],
            serde_json::json!({ "style": "spaces", "width": 4 })
        );
        // None optionals are omitted, not null.
        assert!(v.get("shebang").is_none());
    }
}
