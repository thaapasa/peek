//! The Markdown info section: a Content block (shared text stats) plus a
//! Markdown block, driven by one [`MarkdownView`] that derives both
//! `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView) (themed
//! print). [`MarkdownInfo`] stays the gather struct; the view projects it.
//!
//! The Content block nests under `"text"` in JSON; the Markdown stats flatten
//! into the top-level object. Headings / code blocks / tasks each render as a
//! count row plus indented detail rows (small sub-views) while serializing as
//! their flat numeric fields.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::info::{InfoNode, InfoValue, Value, paint_count, render_info};
use crate::theme::PeekTheme;
use crate::types::markdown::info::{FrontmatterKind, MarkdownInfo, MarkdownStats};
use crate::types::text::info_render::TextView;

/// Themed terminal Markdown section (Content + Markdown blocks).
pub fn render_section(lines: &mut Vec<String>, info: &MarkdownInfo, theme: &PeekTheme) {
    render_info(lines, &MarkdownView::from(info), theme);
}

/// Typed `--info --json` view, nested under `"markdown"`; the text stats nest
/// under `text`.
pub fn json_section(info: &MarkdownInfo) -> (&'static str, serde_json::Value) {
    (
        "markdown",
        serde_json::to_value(MarkdownView::from(info)).expect("markdown info view serializes"),
    )
}

#[derive(Serialize, crate::info::InfoView)]
struct MarkdownView {
    #[info(nest)]
    #[serde(rename = "text")]
    content: TextView,
    #[info(nest)]
    #[serde(flatten)]
    md: MarkdownSection,
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "Markdown")]
struct MarkdownSection {
    #[info(label = "Frontmatter")]
    #[serde(skip_serializing_if = "Option::is_none")]
    frontmatter: Option<FrontmatterKind>,
    #[info(nest)]
    #[serde(flatten)]
    headings: Headings,
    #[info(nest)]
    #[serde(flatten)]
    code: CodeBlocks,
    #[info(label = "Inline Code", skip_if_zero)]
    inline_code_count: Value,
    #[info(label = "Links", skip_if_zero)]
    link_count: Value,
    #[info(label = "Images", skip_if_zero)]
    image_count: Value,
    #[info(label = "Tables", skip_if_zero)]
    table_count: Value,
    #[info(label = "List Items", skip_if_zero)]
    list_item_count: Value,
    #[info(nest)]
    #[serde(flatten)]
    tasks: Tasks,
    #[info(label = "Blockquotes", skip_if_zero)]
    blockquote_lines: Value,
    #[info(label = "Footnotes", skip_if_zero)]
    footnote_def_count: Value,
    #[info(label = "Prose Words")]
    prose_words: Value,
    #[info(label = "Reading Time", skip_if_zero)]
    reading_minutes: ReadingTime,
}

impl From<&MarkdownInfo> for MarkdownView {
    fn from(info: &MarkdownInfo) -> Self {
        let s: &MarkdownStats = &info.stats;
        MarkdownView {
            content: TextView::from(&info.text),
            md: MarkdownSection {
                frontmatter: s.frontmatter,
                headings: Headings(s.heading_counts),
                code: CodeBlocks {
                    count: s.code_block_count,
                    languages: s.code_block_languages.clone(),
                },
                inline_code_count: Value::count(s.inline_code_count as u64),
                link_count: Value::count(s.link_count as u64),
                image_count: Value::count(s.image_count as u64),
                table_count: Value::count(s.table_count as u64),
                list_item_count: Value::count(s.list_item_count as u64),
                tasks: Tasks {
                    done: s.task_done,
                    total: s.task_total,
                },
                blockquote_lines: Value::count(s.blockquote_lines as u64),
                footnote_def_count: Value::count(s.footnote_def_count as u64),
                prose_words: Value::count(s.prose_words as u64),
                reading_minutes: ReadingTime(s.reading_minutes),
            },
        }
    }
}

/// H1..H6 counts. Print: a `Headings` total plus one or two indented
/// per-level rows. JSON: `heading_count` (total) + `heading_counts` (array).
struct Headings([usize; 6]);

impl crate::info::InfoView for Headings {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let total: usize = self.0.iter().sum();
        if total == 0 {
            return Vec::new();
        }
        let mut nodes = vec![
            InfoNode::Row {
                label: "Headings",
                value: paint_count(total, theme),
            },
            InfoNode::Row {
                label: "  H1/H2/H3",
                value: format_levels(&self.0[..3], theme),
            },
        ];
        if self.0[3..].iter().any(|&n| n > 0) {
            nodes.push(InfoNode::Row {
                label: "  H4/H5/H6",
                value: format_levels(&self.0[3..], theme),
            });
        }
        nodes
    }
}

impl Serialize for Headings {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let total: usize = self.0.iter().sum();
        let mut st = ser.serialize_struct("headings", 2)?;
        st.serialize_field("heading_count", &total)?;
        st.serialize_field("heading_counts", &self.0.to_vec())?;
        st.end()
    }
}

/// Fenced-code-block tally. Print: a `Code Blocks` count plus an indented
/// muted `Languages` row. JSON: `code_block_count` + `code_block_languages`.
struct CodeBlocks {
    count: usize,
    languages: Vec<String>,
}

impl crate::info::InfoView for CodeBlocks {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        if self.count == 0 {
            return Vec::new();
        }
        let mut nodes = vec![InfoNode::Row {
            label: "Code Blocks",
            value: paint_count(self.count, theme),
        }];
        if !self.languages.is_empty() {
            nodes.push(InfoNode::Row {
                label: "  Languages",
                value: theme.paint_muted(&self.languages.join(", ")),
            });
        }
        nodes
    }
}

impl Serialize for CodeBlocks {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut st = ser.serialize_struct("code", 2)?;
        st.serialize_field("code_block_count", &self.count)?;
        st.serialize_field("code_block_languages", &self.languages)?;
        st.end()
    }
}

/// Task-list tally. Print: a `Tasks` row `done / total  (pct%)` when any.
/// JSON: `task_done` + `task_total`.
struct Tasks {
    done: usize,
    total: usize,
}

impl crate::info::InfoView for Tasks {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        if self.total == 0 {
            return Vec::new();
        }
        let pct = (self.done as f64 / self.total as f64 * 100.0).round() as u32;
        let value = format!(
            "{} / {}  {}",
            paint_count(self.done, theme),
            paint_count(self.total, theme),
            theme.paint_muted(&format!("({pct}%)"))
        );
        vec![InfoNode::Row {
            label: "Tasks",
            value,
        }]
    }
}

impl Serialize for Tasks {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut st = ser.serialize_struct("tasks", 2)?;
        st.serialize_field("task_done", &self.done)?;
        st.serialize_field("task_total", &self.total)?;
        st.end()
    }
}

/// Reading-time estimate. Print: `N min` (omitted under a minute). JSON: the
/// raw minute count.
struct ReadingTime(u32);

impl InfoValue for ReadingTime {
    fn render_value(&self, theme: &PeekTheme) -> String {
        let label = if self.0 == 1 {
            "1 min".to_string()
        } else {
            format!("{} min", self.0)
        };
        theme.paint_value(&label)
    }
}

impl crate::info::MaybeZero for ReadingTime {
    fn is_zero_value(&self) -> bool {
        self.0 == 0
    }
}

impl Serialize for ReadingTime {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_u32(self.0)
    }
}

impl InfoValue for FrontmatterKind {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_value(match self {
            FrontmatterKind::Yaml => "YAML",
            FrontmatterKind::Toml => "TOML",
        })
    }
}

impl Serialize for FrontmatterKind {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(match self {
            FrontmatterKind::Yaml => "yaml",
            FrontmatterKind::Toml => "toml",
        })
    }
}

/// Per-level counts joined by a muted ` / ` (each count on the count gradient).
fn format_levels(counts: &[usize], theme: &PeekTheme) -> String {
    counts
        .iter()
        .map(|&n| paint_count(n, theme))
        .collect::<Vec<_>>()
        .join(theme.paint_muted(" / ").as_str())
}
