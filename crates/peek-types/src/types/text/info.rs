//! Text statistics shape shared across text-based viewers (plain text,
//! Markdown, SQL, SVG source). Owned here because text is the primary
//! producer; other types import [`TextStats`] when they need to display
//! source-level metrics alongside their own.

pub struct TextStats {
    pub line_count: usize,
    pub word_count: usize,
    pub char_count: usize,
    pub blank_lines: usize,
    pub longest_line_chars: usize,
    pub line_endings: LineEndings,
    pub indent_style: Option<IndentStyle>,
    pub encoding: Encoding,
    pub shebang: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LineEndings {
    None,
    Lf,
    Crlf,
    Cr,
    Mixed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Tabs,
    Spaces(u8),
    Mixed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}
