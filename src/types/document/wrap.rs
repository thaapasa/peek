//! Shared word-wrap + SGR-bracketing helpers used by both the DOCX/ODT
//! renderer ([`super::render`]) and the RTF renderer
//! ([`super::rtf::render`]). The two wrap engines stay branched —
//! their input shapes (paragraph-with-runs tree vs flat painter-tagged
//! stream) are real — but the per-word tokenizer, display-width helper,
//! and SGR open/close bracketing are identical and live here.

use syntect::highlighting::Color;

use crate::theme::{Attr, StyleMode};

/// Run-style attributes shared by both AST shapes (DOCX run, RTF block
/// painter). Built per emission by the caller and consumed by
/// [`emit_styled`].
#[derive(Copy, Clone, Default)]
pub(crate) struct SgrStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<Color>,
}

/// Write `text` to `out` wrapped in SGR open/close sequences for the
/// flags + color set on `style`. No-op when `text` is empty so call
/// sites don't need to guard.
pub(crate) fn emit_styled(text: &str, style: SgrStyle, mode: StyleMode, out: &mut String) {
    if text.is_empty() {
        return;
    }
    if style.bold {
        out.push_str(mode.attr_open(Attr::Bold));
    }
    if style.italic {
        out.push_str(mode.attr_open(Attr::Italic));
    }
    if style.underline {
        out.push_str(mode.attr_open(Attr::Underline));
    }
    if style.strike {
        out.push_str(mode.attr_open(Attr::Strikeout));
    }
    if let Some(c) = style.color {
        mode.write_fg_seq(out, c);
    }
    out.push_str(text);
    if style.color.is_some() {
        out.push_str(mode.reset_fg());
    }
    if style.strike {
        out.push_str(mode.attr_close(Attr::Strikeout));
    }
    if style.underline {
        out.push_str(mode.attr_close(Attr::Underline));
    }
    if style.italic {
        out.push_str(mode.attr_close(Attr::Italic));
    }
    if style.bold {
        out.push_str(mode.attr_close(Attr::Bold));
    }
}

/// Split `s` into alternating whitespace / non-whitespace runs. Each
/// resulting `String` is either fully whitespace or fully
/// non-whitespace, preserving original characters. Empty input yields
/// an empty vec.
pub(crate) fn split_words(s: &str) -> Vec<String> {
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

/// Approximate display width via `unicode-width`. Good enough for v1
/// terminal wrap — matches the columns used to compose the line.
pub(crate) fn visible_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    UnicodeWidthStr::width(s)
}
