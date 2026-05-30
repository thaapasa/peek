//! `TextRenderer` for Jupyter notebooks.
//!
//! Strategy: **translate the notebook to one Markdown document, then
//! render it through the existing Markdown pipeline.** A notebook is, by
//! design, prose cells (already Markdown) interleaved with
//! syntax-highlighted code and its textual output — exactly what the
//! Markdown renderer already produces (headings, wrapping, fenced-code
//! syntect highlight, theme integration). Synthesising a Markdown string
//! and reusing `markdown::render` buys all of that for ~one screen of
//! translation code instead of re-implementing highlighting + wrapping.
//!
//! Code cells become fenced blocks tagged with the kernel language;
//! markdown cells pass through verbatim; text/stream/error outputs
//! become fenced output blocks. Rich image outputs are noted, not drawn
//! — inline ASCII image rendering inside a scrolling text document is a
//! follow-up (the Markdown renderer has no inline-image concept either).

use std::rc::Rc;

use anyhow::Result;

use crate::input::InputSource;
use crate::theme::{PeekTheme, PeekThemeName, StyleMode, ThemeManager};
use crate::types::markdown::render_markdown;
use crate::viewer::modes::{ModeId, TextRenderer};

use super::model::{Cell, CellKind, Notebook, Output};

pub(crate) struct NotebookRenderer {
    source: InputSource,
    theme_manager: Rc<ThemeManager>,
}

impl NotebookRenderer {
    pub(crate) fn new(source: InputSource, theme_manager: Rc<ThemeManager>) -> Self {
        Self {
            source,
            theme_manager,
        }
    }
}

impl TextRenderer for NotebookRenderer {
    fn label(&self) -> &'static str {
        "Rendered"
    }

    fn mode_id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn render(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        theme_name: PeekThemeName,
        style_mode: StyleMode,
    ) -> Result<Vec<String>> {
        let text = self.source.read_text()?;
        let md = match Notebook::parse(&text) {
            Some(nb) => to_markdown(&nb),
            // Not a parseable notebook — fall back to showing the raw
            // JSON so the rendered view is never blank.
            None => text,
        };
        render_markdown(
            &md,
            width,
            theme,
            style_mode,
            &self.theme_manager,
            theme_name,
        )
    }
}

/// Translate a parsed notebook into a single Markdown document.
fn to_markdown(nb: &Notebook) -> String {
    let lang = nb.language.as_deref().unwrap_or("");
    let mut md = String::new();
    for (i, cell) in nb.cells.iter().enumerate() {
        if i > 0 {
            md.push_str("\n---\n\n");
        }
        emit_cell(&mut md, cell, lang);
    }
    md
}

fn emit_cell(md: &mut String, cell: &Cell, lang: &str) {
    match cell.kind {
        CellKind::Markdown => {
            md.push_str(cell.source.trim_end());
            md.push('\n');
        }
        CellKind::Raw => {
            push_fence(md, &cell.source, "");
        }
        CellKind::Code => {
            let label = match cell.exec_count {
                Some(n) => format!("**In [{n}]:**\n\n"),
                None => "**In [ ]:**\n\n".to_string(),
            };
            md.push_str(&label);
            push_fence(md, &cell.source, lang);
            for out in &cell.outputs {
                emit_output(md, out);
            }
        }
    }
}

fn emit_output(md: &mut String, out: &Output) {
    match out {
        Output::Stream { stderr, text } => {
            md.push_str(if *stderr { "\n*stderr:*\n\n" } else { "\n" });
            push_fence(md, text, "");
        }
        Output::Text(text) => {
            md.push_str("\n*Out:*\n\n");
            push_fence(md, text, "");
        }
        Output::Error {
            ename,
            evalue,
            traceback,
        } => {
            md.push_str(&format!("\n**{ename}: {evalue}**\n\n"));
            let body = if traceback.is_empty() {
                format!("{ename}: {evalue}")
            } else {
                traceback.clone()
            };
            push_fence(md, &body, "");
        }
        Output::Image { mime } => {
            md.push_str(&format!("\n> 🖼 *{mime} output*\n"));
        }
        Output::Html => {
            md.push_str("\n> *text/html output*\n");
        }
    }
}

/// Append `body` as a fenced code block tagged `lang`. The fence is
/// grown longer than any backtick run inside `body` so source/output
/// containing ``` cannot break out of the block.
fn push_fence(md: &mut String, body: &str, lang: &str) {
    let longest = body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    md.push_str(&fence);
    md.push_str(lang);
    md.push('\n');
    md.push_str(body.trim_end_matches('\n'));
    md.push('\n');
    md.push_str(&fence);
    md.push('\n');
}
