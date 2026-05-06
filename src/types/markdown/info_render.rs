use crate::info::{FrontmatterKind, MarkdownStats, paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

pub fn render_section(lines: &mut Vec<String>, stats: &MarkdownStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Markdown", theme);

    if let Some(kind) = stats.frontmatter {
        let label = match kind {
            FrontmatterKind::Yaml => "YAML",
            FrontmatterKind::Toml => "TOML",
        };
        push_field(lines, "Frontmatter", &theme.paint_value(label), theme);
    }

    let total_headings: usize = stats.heading_counts.iter().sum();
    if total_headings > 0 {
        push_field(
            lines,
            "Headings",
            &paint_count(total_headings, theme),
            theme,
        );
        push_field(
            lines,
            "  H1/H2/H3",
            &format_levels(&stats.heading_counts[..3], theme),
            theme,
        );
        if stats.heading_counts[3..].iter().any(|&n| n > 0) {
            push_field(
                lines,
                "  H4/H5/H6",
                &format_levels(&stats.heading_counts[3..], theme),
                theme,
            );
        }
    }

    if stats.code_block_count > 0 {
        push_field(
            lines,
            "Code Blocks",
            &paint_count(stats.code_block_count, theme),
            theme,
        );
        if !stats.code_block_languages.is_empty() {
            push_field(
                lines,
                "  Languages",
                &theme.paint_muted(&stats.code_block_languages.join(", ")),
                theme,
            );
        }
    }
    if stats.inline_code_count > 0 {
        push_field(
            lines,
            "Inline Code",
            &paint_count(stats.inline_code_count, theme),
            theme,
        );
    }

    if stats.link_count > 0 {
        push_field(lines, "Links", &paint_count(stats.link_count, theme), theme);
    }
    if stats.image_count > 0 {
        push_field(
            lines,
            "Images",
            &paint_count(stats.image_count, theme),
            theme,
        );
    }
    if stats.table_count > 0 {
        push_field(
            lines,
            "Tables",
            &paint_count(stats.table_count, theme),
            theme,
        );
    }
    if stats.list_item_count > 0 {
        push_field(
            lines,
            "List Items",
            &paint_count(stats.list_item_count, theme),
            theme,
        );
    }
    if stats.task_total > 0 {
        let done = paint_count(stats.task_done, theme);
        let total = paint_count(stats.task_total, theme);
        let pct = (stats.task_done as f64 / stats.task_total as f64 * 100.0).round() as u32;
        let pct_painted = theme.paint_muted(&format!("({pct}%)"));
        push_field(
            lines,
            "Tasks",
            &format!("{done} / {total}  {pct_painted}"),
            theme,
        );
    }
    if stats.blockquote_lines > 0 {
        push_field(
            lines,
            "Blockquotes",
            &paint_count(stats.blockquote_lines, theme),
            theme,
        );
    }
    if stats.footnote_def_count > 0 {
        push_field(
            lines,
            "Footnotes",
            &paint_count(stats.footnote_def_count, theme),
            theme,
        );
    }

    push_field(
        lines,
        "Prose Words",
        &paint_count(stats.prose_words, theme),
        theme,
    );
    if stats.reading_minutes > 0 {
        let label = if stats.reading_minutes == 1 {
            "1 min".to_string()
        } else {
            format!("{} min", stats.reading_minutes)
        };
        push_field(lines, "Reading Time", &theme.paint_value(&label), theme);
    }
}

fn format_levels(counts: &[usize], theme: &PeekTheme) -> String {
    counts
        .iter()
        .map(|&n| paint_count(n, theme))
        .collect::<Vec<_>>()
        .join(theme.paint_muted(" / ").as_str())
}
