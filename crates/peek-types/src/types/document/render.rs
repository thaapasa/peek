//! Walk a [`super::ast::Doc`] and emit ANSI-styled lines. Output shape
//! (`render(&Doc, width, theme, style_mode) -> Result<Vec<String>>`)
//! mirrors `crate::types::html::render::render` so the read-mode wrapper
//! can cache by `(width, style_mode)` exactly as the EPUB / HTML modes
//! do.

use anyhow::Result;
use peek_theme::{PeekTheme, StyleMode};
use syntect::highlighting::Color;

use crate::types::document::ast::{Block, Doc, Paragraph, Run};
use crate::types::document::wrap::{SgrStyle, emit_styled, split_words, visible_width};

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
        emit_runs(
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
        emit_styled(&segment, run_style(run, false, theme), style_mode, &mut out);
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

fn emit_runs(
    runs: &[Run],
    heading: bool,
    theme: &PeekTheme,
    style_mode: StyleMode,
    out: &mut String,
) {
    for run in runs {
        emit_styled(&run.text, run_style(run, heading, theme), style_mode, out);
    }
}

fn run_style(run: &Run, heading: bool, theme: &PeekTheme) -> SgrStyle {
    let color = run
        .color
        .map(|[r, g, b]| Color { r, g, b, a: 255 })
        .or(if heading { Some(theme.heading) } else { None });
    SgrStyle {
        bold: run.bold || heading,
        italic: run.italic,
        underline: run.underline,
        strike: run.strike,
        color,
    }
}
