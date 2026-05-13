//! Markdown info shape: text-stats sidecar plus Markdown-specific
//! scanner output (heading counts, code blocks, links, tasks, prose word
//! count, reading-time estimate, etc.).

use crate::types::text::info::TextStats;

pub struct MarkdownInfo {
    pub text: TextStats,
    pub stats: MarkdownStats,
}

pub struct MarkdownStats {
    /// Counts for H1..H6 (index 0 = H1).
    pub heading_counts: [usize; 6],
    pub code_block_count: usize,
    /// Distinct fenced-code-block languages, in first-seen order.
    pub code_block_languages: Vec<String>,
    pub inline_code_count: usize,
    pub link_count: usize,
    pub image_count: usize,
    pub table_count: usize,
    pub list_item_count: usize,
    pub task_done: usize,
    pub task_total: usize,
    pub blockquote_lines: usize,
    pub footnote_def_count: usize,
    pub frontmatter: Option<FrontmatterKind>,
    /// Words outside fenced code blocks. Inline code spans aren't stripped
    /// (they usually carry meaningful content for prose).
    pub prose_words: usize,
    /// Reading time at 230 wpm, rounded up to whole minutes (0 = under 1 min).
    pub reading_minutes: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrontmatterKind {
    Yaml,
    Toml,
}
