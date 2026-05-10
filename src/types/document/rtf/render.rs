//! Walk the [`super::parse::Parsed`] AST and emit ANSI-styled lines.
//! Mirrors the DOCX renderer's `render(...)` shape: width-aware wrap,
//! per-run SGR, returns a `Vec<String>` for the read-mode cache.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::theme::{Attr, PeekTheme, StyleMode};
use crate::types::document::rtf::parse::{BlockPainter, Parsed};

pub(crate) fn render(
    parsed: &Parsed,
    width: usize,
    theme: &PeekTheme,
    style_mode: StyleMode,
) -> Result<Vec<String>> {
    let _ = theme;
    let width = width.max(20);

    let mut out: Vec<String> = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0usize;

    for block in &parsed.blocks {
        // RTF style blocks chain inline; newlines inside the block
        // text mark line breaks (the parser inserts `\n` for `\par`
        // / `\line` / CRLF).
        for piece in split_keep_newlines(&block.text) {
            if piece == "\n" {
                out.push(std::mem::take(&mut current_line));
                current_width = 0;
                continue;
            }
            for word in split_words(piece) {
                let visible = visible_width(&word);
                let is_ws = word.chars().all(char::is_whitespace);
                if !is_ws && current_width + visible > width && current_width > 0 {
                    out.push(std::mem::take(&mut current_line));
                    current_width = 0;
                }
                if is_ws && current_width == 0 {
                    continue;
                }
                emit(&block.painter, &word, style_mode, &mut current_line);
                current_width += visible;
            }
        }
    }
    if !current_line.is_empty() {
        out.push(current_line);
    }
    Ok(out)
}

fn emit(painter: &BlockPainter, text: &str, style_mode: StyleMode, out: &mut String) {
    if text.is_empty() {
        return;
    }
    if painter.bold {
        out.push_str(style_mode.attr_open(Attr::Bold));
    }
    if painter.italic {
        out.push_str(style_mode.attr_open(Attr::Italic));
    }
    if painter.underline {
        out.push_str(style_mode.attr_open(Attr::Underline));
    }
    if painter.strike {
        out.push_str(style_mode.attr_open(Attr::Strikeout));
    }
    if let Some([r, g, b]) = painter.color {
        style_mode.write_fg_seq(out, Color { r, g, b, a: 255 });
    }
    out.push_str(text);
    if painter.color.is_some() {
        out.push_str(style_mode.reset_fg());
    }
    if painter.strike {
        out.push_str(style_mode.attr_close(Attr::Strikeout));
    }
    if painter.underline {
        out.push_str(style_mode.attr_close(Attr::Underline));
    }
    if painter.italic {
        out.push_str(style_mode.attr_close(Attr::Italic));
    }
    if painter.bold {
        out.push_str(style_mode.attr_close(Attr::Bold));
    }
}

fn split_keep_newlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut last = 0;
    for (i, _) in s.match_indices('\n') {
        if i > last {
            out.push(&s[last..i]);
        }
        out.push("\n");
        last = i + 1;
    }
    if last < s.len() {
        out.push(&s[last..]);
    }
    out
}

fn split_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_ws = false;
    for ch in s.chars() {
        let is_ws = ch.is_whitespace();
        if is_ws != in_ws && !buf.is_empty() {
            out.push(std::mem::take(&mut buf));
        }
        buf.push(ch);
        in_ws = is_ws;
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

fn visible_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    UnicodeWidthStr::width(s)
}
