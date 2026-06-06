//! Render the notebook Info section.

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::info::NotebookInfo;

pub fn render_section(lines: &mut Vec<String>, info: &NotebookInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Notebook", theme);

    let (major, minor) = info.nbformat;
    push_field(
        lines,
        "Format",
        &theme.paint_value(&format!("nbformat {major}.{minor}")),
        theme,
    );

    if let Some(kernel) = &info.kernel {
        push_field(lines, "Kernel", &theme.paint_value(kernel), theme);
    }
    if let Some(lang) = &info.language {
        let label = match &info.language_version {
            Some(v) => format!("{lang} {v}"),
            None => lang.clone(),
        };
        push_field(lines, "Language", &theme.paint_value(&label), theme);
    }

    push_field(
        lines,
        "Cells",
        &paint_count(info.cell_count(), theme),
        theme,
    );
    push_field(
        lines,
        "  Code/Markdown",
        &format!(
            "{}{}{}",
            paint_count(info.code_cells, theme),
            theme.paint_muted(" / "),
            paint_count(info.markdown_cells, theme),
        ),
        theme,
    );
    if info.raw_cells > 0 {
        push_field(lines, "  Raw", &paint_count(info.raw_cells, theme), theme);
    }

    if info.output_count > 0 {
        push_field(
            lines,
            "Outputs",
            &paint_count(info.output_count, theme),
            theme,
        );
        if info.image_outputs > 0 {
            push_field(
                lines,
                "  Images",
                &paint_count(info.image_outputs, theme),
                theme,
            );
        }
        if info.error_outputs > 0 {
            push_field(
                lines,
                "  Errors",
                &paint_count(info.error_outputs, theme),
                theme,
            );
        }
    }

    if let Some(n) = info.max_exec_count {
        push_field(
            lines,
            "Max Execution",
            &theme.paint_value(&format!("[{n}]")),
            theme,
        );
    }
}

/// Typed `--info --json` encoding of the Notebook section. Counts and the
/// nbformat version are raw numbers; optional metadata is omitted when absent.
pub fn json_section(info: &NotebookInfo) -> (&'static str, serde_json::Value) {
    let (major, minor) = info.nbformat;
    let mut obj = serde_json::json!({
        "nbformat_major": major,
        "nbformat_minor": minor,
        "cell_count": info.cell_count(),
        "code_cells": info.code_cells,
        "markdown_cells": info.markdown_cells,
        "raw_cells": info.raw_cells,
        "output_count": info.output_count,
        "image_outputs": info.image_outputs,
        "error_outputs": info.error_outputs,
    });
    if let Some(ref kernel) = info.kernel {
        obj["kernel"] = serde_json::json!(kernel);
    }
    if let Some(ref lang) = info.language {
        obj["language"] = serde_json::json!(lang);
    }
    if let Some(ref version) = info.language_version {
        obj["language_version"] = serde_json::json!(version);
    }
    if let Some(n) = info.max_exec_count {
        obj["max_exec_count"] = serde_json::json!(n);
    }
    ("notebook", obj)
}
