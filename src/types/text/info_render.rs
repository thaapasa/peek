use crate::info::{Encoding, IndentStyle, LineEndings, TextStats, paint_count, push_field};
use crate::theme::PeekTheme;

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
