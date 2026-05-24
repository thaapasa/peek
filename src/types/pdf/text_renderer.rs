//! `TextRenderer` for PDF text extraction.
//!
//! Renders the whole document to a width-wrapped `Vec<String>`. Page
//! boundaries get a muted `--- Page N ---` separator line so the
//! reader has a visual anchor without leaving the single scroll axis.
//! Per-page extraction failures degrade to a placeholder line and a
//! warning rather than killing the whole render. The generic
//! [`RenderedTextMode`] supplies caching, search, and windowing.

use anyhow::Result;

use crate::theme::{PeekTheme, PeekThemeName, StyleMode};
use crate::viewer::modes::{ModeId, TextRenderer};

use super::package::Doc;

pub(crate) struct PdfTextRenderer {
    doc: Doc,
    /// Warnings from the most recent render — per-page extract failures.
    /// Drained by `take_warnings`; cleared at the start of each render.
    warnings: Vec<String>,
}

impl PdfTextRenderer {
    pub(crate) fn new(doc: Doc) -> Self {
        Self {
            doc,
            warnings: Vec::new(),
        }
    }
}

impl TextRenderer for PdfTextRenderer {
    fn label(&self) -> &'static str {
        "Text"
    }

    fn mode_id(&self) -> ModeId {
        // PDF text is the document's content view, not a styled render.
        ModeId::Content
    }

    fn render(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        _theme_name: PeekThemeName,
        _style_mode: StyleMode,
    ) -> Result<Vec<String>> {
        self.warnings.clear();
        Ok(render_text(&self.doc, width, theme, &mut self.warnings))
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

/// Wrap each page's text at `width`, separated by a muted page marker.
/// Errors per page degrade to a single placeholder line so a corrupt
/// page doesn't kill the whole render.
fn render_text(
    doc: &Doc,
    width: usize,
    theme: &PeekTheme,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    let total = doc.page_count();
    for idx in 0..total {
        if idx > 0 {
            out.push(String::new());
            out.push(theme.paint_muted(&format!("--- Page {} ---", idx + 1)));
            out.push(String::new());
        }
        match doc.page_text(idx) {
            Ok(text) => {
                for line in text.lines() {
                    push_wrapped(&mut out, line, width.max(20));
                }
            }
            Err(e) => {
                warnings.push(format!("page {}: text extract failed: {e:#}", idx + 1));
                out.push(theme.paint_warning(&format!("[page {} text unavailable]", idx + 1)));
            }
        }
    }
    out
}

/// Greedy word-wrap by character count. Falls back to mid-word breaks
/// for tokens longer than `width` so a giant URL or compound word
/// doesn't overflow the terminal line.
fn push_wrapped(out: &mut Vec<String>, line: &str, width: usize) {
    if line.is_empty() {
        out.push(String::new());
        return;
    }
    let mut buf = String::new();
    let mut col = 0usize;
    for word in line.split_whitespace() {
        let word_len = word.chars().count();
        if word_len > width {
            if !buf.is_empty() {
                out.push(std::mem::take(&mut buf));
                col = 0;
            }
            // Hard-split the long word at width boundaries.
            let mut chars = word.chars();
            loop {
                let chunk: String = chars.by_ref().take(width).collect();
                if chunk.is_empty() {
                    break;
                }
                out.push(chunk);
            }
            continue;
        }
        let needed = if col == 0 { word_len } else { word_len + 1 };
        if col + needed > width {
            out.push(std::mem::take(&mut buf));
            col = 0;
        }
        if col > 0 {
            buf.push(' ');
            col += 1;
        }
        buf.push_str(word);
        col += word_len;
    }
    if !buf.is_empty() {
        out.push(buf);
    }
}
