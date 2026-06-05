//! Shared word-processing document AST. Produced by per-format parsers
//! ([`super::docx::package`], [`super::odt::package`]) and consumed by
//! the shared [`super::render`] + [`super::read_mode`] pair so each new
//! container format only needs a parser, not a renderer.
//!
//! The AST is intentionally coarse: paragraphs, tables, runs. Anything
//! the format carries beyond that (revision marks, footnotes, comments,
//! complex frames) collapses to either a paragraph or a styled run.

use super::DocumentMetadata;

/// Owned document representation. No references back to the source
/// container; safe to keep across renders.
pub struct Doc {
    pub metadata: DocumentMetadata,
    pub blocks: Vec<Block>,
    pub paragraph_count: usize,
    pub word_count: usize,
    pub image_count: usize,
}

/// One body-level block. Tables flatten to row-of-cell-of-paragraphs.
pub enum Block {
    Paragraph(Paragraph),
    Table(Vec<Vec<Paragraph>>),
}

#[derive(Default)]
pub struct Paragraph {
    /// Heading level 1..=6 when the paragraph is a heading; `None` for
    /// body paragraphs.
    pub heading_level: Option<u8>,
    /// List bullet / numbering prefix when the paragraph is a list
    /// item. Numbering cascades aren't resolved; everything bullets as
    /// "•". `indent_level` controls leading whitespace.
    pub list_marker: Option<String>,
    /// Indent level for nested lists (0 = top level).
    pub indent_level: u8,
    pub runs: Vec<Run>,
}

#[derive(Default, Clone)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<[u8; 3]>,
}

pub fn count_words(runs: &[Run]) -> usize {
    runs.iter()
        .flat_map(|r| r.text.split_whitespace())
        .filter(|w| !w.is_empty())
        .count()
}

pub fn merge_paragraphs(paragraphs: Vec<Paragraph>) -> Paragraph {
    let mut out = Paragraph::default();
    for (i, p) in paragraphs.into_iter().enumerate() {
        if i > 0 && !out.runs.is_empty() {
            out.runs.push(Run {
                text: " ".to_string(),
                ..Run::default()
            });
        }
        out.runs.extend(p.runs);
    }
    out
}
