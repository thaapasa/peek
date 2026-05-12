//! Walk a [`super::ast::Doc`] and emit ANSI-styled lines. Output shape
//! (`render(&Doc, width, theme, style_mode) -> Result<Vec<String>>`)
//! mirrors `crate::types::html::render::render` so the read-mode wrapper
//! can cache by `(width, style_mode)` exactly as the EPUB / HTML modes
//! do.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::theme::{Attr, PeekTheme, StyleMode};
use crate::types::document::ast::{Block, Doc, Paragraph, Run};

/// Render an in-memory document AST to ANSI-styled lines.
pub fn render(
    doc: &Doc,
    width: usize,
    theme: &PeekTheme,
    style_mode: StyleMode,
) -> Result<Vec<String>> {
    let width = width.max(20);
    let mut out: Vec<String> = Vec::new();
    for (i, block) in doc.blocks.iter().enumerate() {
        match block {
            Block::Paragraph(p) => {
                if i > 0 && p.heading_level.is_some() {
                    out.push(String::new());
                }
                render_paragraph(p, width, theme, style_mode, &mut out);
            }
            Block::Table(rows) => {
                for row in rows {
                    let cells: Vec<String> = row
                        .iter()
                        .map(|cell| flatten_runs(&cell.runs, theme, style_mode))
                        .collect();
                    out.push(cells.join(" | "));
                }
                out.push(String::new());
            }
        }
    }
    Ok(out)
}

fn render_paragraph(
    p: &Paragraph,
    width: usize,
    theme: &PeekTheme,
    style_mode: StyleMode,
    out: &mut Vec<String>,
) {
    let leading_indent = " ".repeat((p.indent_level as usize) * 2);
    let prefix = if let Some(marker) = &p.list_marker {
        format!("{leading_indent}{marker} ")
    } else if p.heading_level.is_some() {
        String::new()
    } else {
        leading_indent.clone()
    };

    let prefix_len = visible_width(&prefix);
    let body_width = width.saturating_sub(prefix_len).max(8);

    let chunks = wrap_runs(&p.runs, body_width);
    if chunks.is_empty() {
        out.push(String::new());
        return;
    }

    let continuation = " ".repeat(prefix_len);
    for (i, line_runs) in chunks.iter().enumerate() {
        let mut line = String::new();
        if i == 0 {
            line.push_str(&prefix);
        } else {
            line.push_str(&continuation);
        }
        emit_styled(
            line_runs,
            p.heading_level.is_some(),
            theme,
            style_mode,
            &mut line,
        );
        out.push(line);
    }
}

/// Concatenate runs as a single space-joined line (no wrap). Used for
/// table cells.
fn flatten_runs(runs: &[Run], theme: &PeekTheme, style_mode: StyleMode) -> String {
    let mut out = String::new();
    for run in runs {
        let segment = run.text.replace('\n', " ");
        if segment.is_empty() {
            continue;
        }
        emit_run(run, &segment, false, theme, style_mode, &mut out);
    }
    out
}

/// Word-wrap a run sequence onto multiple "soft lines". Each output
/// item is a Vec<Run> covering one wrapped line; styling carries
/// across the wrap.
fn wrap_runs(runs: &[Run], width: usize) -> Vec<Vec<Run>> {
    let mut lines: Vec<Vec<Run>> = Vec::new();
    let mut current: Vec<Run> = Vec::new();
    let mut current_width = 0usize;

    for run in runs {
        // Hard-wrap on embedded newlines (paragraph-internal breaks).
        let segments: Vec<&str> = run.text.split('\n').collect();
        for (i, segment) in segments.iter().enumerate() {
            if i > 0 {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            for word in split_words(segment) {
                let w = visible_width(&word);
                let need_space = !current.is_empty()
                    && !word.starts_with(char::is_whitespace)
                    && !current
                        .last()
                        .map(|r| r.text.ends_with(char::is_whitespace))
                        .unwrap_or(true);
                let advance = if need_space { w + 1 } else { w };
                if current_width + advance > width && current_width > 0 {
                    lines.push(std::mem::take(&mut current));
                    current_width = 0;
                }
                if word.chars().all(char::is_whitespace) && current.is_empty() {
                    continue;
                }
                let mut piece = run.clone();
                if !current.is_empty()
                    && !word.starts_with(char::is_whitespace)
                    && !current
                        .last()
                        .map(|r| r.text.ends_with(char::is_whitespace))
                        .unwrap_or(true)
                {
                    piece.text = format!(" {word}");
                    current_width += w + 1;
                } else {
                    piece.text = word.to_string();
                    current_width += w;
                }
                current.push(piece);
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
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

fn emit_styled(
    runs: &[Run],
    heading: bool,
    theme: &PeekTheme,
    style_mode: StyleMode,
    out: &mut String,
) {
    for run in runs {
        emit_run(run, &run.text, heading, theme, style_mode, out);
    }
}

fn emit_run(
    run: &Run,
    text: &str,
    heading: bool,
    theme: &PeekTheme,
    style_mode: StyleMode,
    out: &mut String,
) {
    if text.is_empty() {
        return;
    }
    let bold = run.bold || heading;
    if bold {
        out.push_str(style_mode.attr_open(Attr::Bold));
    }
    if run.italic {
        out.push_str(style_mode.attr_open(Attr::Italic));
    }
    if run.underline {
        out.push_str(style_mode.attr_open(Attr::Underline));
    }
    if run.strike {
        out.push_str(style_mode.attr_open(Attr::Strikeout));
    }
    let color = run
        .color
        .map(|[r, g, b]| Color { r, g, b, a: 255 })
        .or(if heading { Some(theme.heading) } else { None });
    if let Some(c) = color {
        style_mode.write_fg_seq(out, c);
    }
    out.push_str(text);
    if color.is_some() {
        out.push_str(style_mode.reset_fg());
    }
    if run.strike {
        out.push_str(style_mode.attr_close(Attr::Strikeout));
    }
    if run.underline {
        out.push_str(style_mode.attr_close(Attr::Underline));
    }
    if run.italic {
        out.push_str(style_mode.attr_close(Attr::Italic));
    }
    if bold {
        out.push_str(style_mode.attr_close(Attr::Bold));
    }
}

/// Approximate display width — `unicode-width` already in deps; ASCII
/// good enough for a v1 wrap.
fn visible_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    UnicodeWidthStr::width(s)
}
