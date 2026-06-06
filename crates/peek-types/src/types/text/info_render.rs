use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;
use crate::types::text::info::{Encoding, IndentStyle, LineEndings, TextStats};

/// Render the standalone Content section for plain text files.
pub fn render_section(lines: &mut Vec<String>, stats: &TextStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Content", theme);
    push_text_stats(lines, stats, theme);
}

pub fn push_text_stats(lines: &mut Vec<String>, stats: &TextStats, theme: &PeekTheme) {
    push_field(lines, "Lines", &paint_count(stats.line_count, theme), theme);
    if stats.blank_lines > 0 {
        push_field(
            lines,
            "Blank Lines",
            &paint_count(stats.blank_lines, theme),
            theme,
        );
    }
    push_field(lines, "Words", &paint_count(stats.word_count, theme), theme);
    push_field(
        lines,
        "Characters",
        &paint_count(stats.char_count, theme),
        theme,
    );
    if stats.longest_line_chars > 0 {
        push_field(
            lines,
            "Longest Line",
            &paint_count(stats.longest_line_chars, theme),
            theme,
        );
    }
    push_field(
        lines,
        "Line Endings",
        &theme.paint_value(line_endings_label(stats.line_endings)),
        theme,
    );
    if let Some(indent) = stats.indent_style {
        push_field(
            lines,
            "Indent",
            &theme.paint_value(&indent_label(indent)),
            theme,
        );
    }
    push_field(
        lines,
        "Encoding",
        &theme.paint_muted(encoding_label(stats.encoding)),
        theme,
    );
    if let Some(shebang) = &stats.shebang {
        push_field(lines, "Shebang", &theme.paint_value(shebang), theme);
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

/// Typed `--info --json` encoding of the Content section. Enum fields use
/// stable lowercase machine tokens rather than the display labels.
pub fn json_section(stats: &TextStats) -> (&'static str, serde_json::Value) {
    let mut obj = serde_json::json!({
        "line_count": stats.line_count,
        "word_count": stats.word_count,
        "char_count": stats.char_count,
        "blank_lines": stats.blank_lines,
        "longest_line_chars": stats.longest_line_chars,
        "line_endings": line_endings_token(stats.line_endings),
        "encoding": encoding_token(stats.encoding),
    });
    if let Some(indent) = stats.indent_style {
        obj["indent"] = indent_json(indent);
    }
    if let Some(ref shebang) = stats.shebang {
        obj["shebang"] = serde_json::json!(shebang);
    }
    ("text", obj)
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

fn encoding_token(enc: Encoding) -> &'static str {
    match enc {
        Encoding::Utf8 => "utf-8",
        Encoding::Utf8Bom => "utf-8-bom",
        Encoding::Utf16Le => "utf-16-le",
        Encoding::Utf16Be => "utf-16-be",
    }
}

fn indent_json(style: IndentStyle) -> serde_json::Value {
    match style {
        IndentStyle::Tabs => serde_json::json!({ "style": "tabs" }),
        IndentStyle::Spaces(n) => serde_json::json!({ "style": "spaces", "width": n }),
        IndentStyle::Mixed => serde_json::json!({ "style": "mixed" }),
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
