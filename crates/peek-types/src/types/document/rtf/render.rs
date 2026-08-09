//! Walk the [`super::parse::Parsed`] AST and emit ANSI-styled lines.
//! Mirrors the DOCX renderer's `render(...)` shape: width-aware wrap,
//! per-run SGR, returns a `Vec<String>` for the read-mode cache.

use anyhow::Result;
use peek_theme::{PeekTheme, StyleMode};
use syntect::highlighting::Color;

use crate::types::document::rtf::parse::{BlockPainter, Parsed};
use crate::types::document::wrap::{SgrStyle, emit_styled, split_words, visible_width};

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
        let style = painter_style(&block.painter);
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
                emit_styled(&word, style, style_mode, &mut current_line);
                current_width += visible;
            }
        }
    }
    if !current_line.is_empty() {
        out.push(current_line);
    }
    Ok(out)
}

fn painter_style(painter: &BlockPainter) -> SgrStyle {
    SgrStyle {
        bold: painter.bold,
        italic: painter.italic,
        underline: painter.underline,
        strike: painter.strike,
        color: painter.color.map(|[r, g, b]| Color { r, g, b, a: 255 }),
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
