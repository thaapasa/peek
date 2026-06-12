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
use crate::viewer::modes::{ModeId, TextRenderer, render_cap_placeholder};

use super::model::{Cell, CellKind, Notebook, Output};

pub(crate) struct NotebookRenderer {
    source: InputSource,
    theme_manager: Rc<ThemeManager>,
    warning: Option<String>,
}

impl NotebookRenderer {
    pub(crate) fn new(source: InputSource, theme_manager: Rc<ThemeManager>) -> Self {
        Self {
            source,
            theme_manager,
            warning: None,
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
        // Whole-notebook parse + markdown synthesis — gate like the
        // other whole-document renderers (HTML / markdown).
        let len = self.source.byte_len()?;
        if let Some(lines) = render_cap_placeholder(len, "notebook", &mut self.warning) {
            return Ok(lines);
        }
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

    fn take_warnings(&mut self) -> Vec<String> {
        self.warning.take().into_iter().collect()
    }
}

/// Translate a parsed notebook into a single Markdown document.
fn to_markdown(nb: &Notebook) -> String {
    let lang = nb.language.as_deref().unwrap_or("");
    let mut md = String::new();
    // Image outputs are numbered in document order to match the Blocks
    // listing, so a `🖼 image-1.png` note maps to the `image-1.png` row.
    let mut image_seq = 0;
    for (i, cell) in nb.cells.iter().enumerate() {
        if i > 0 {
            md.push_str("\n---\n\n");
        }
        emit_cell(&mut md, cell, lang, &mut image_seq);
    }
    md
}

fn emit_cell(md: &mut String, cell: &Cell, lang: &str, image_seq: &mut usize) {
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
                emit_output(md, out, image_seq);
            }
        }
    }
}

fn emit_output(md: &mut String, out: &Output, image_seq: &mut usize) {
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
            *image_seq += 1;
            let name = super::listing::image_name(*image_seq, mime);
            // NBSP (U+00A0) + space: the Markdown renderer collapses runs
            // of ASCII whitespace to one (HTML text semantics) and the wide
            // image glyph visually swallows the survivor. NBSP is not
            // collapsed, so the gap holds; the trailing space widens it to
            // a clear separation before the name.
            md.push_str(&format!("\n> 🖼\u{00a0} *{name}*\n"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::modes::RENDER_MAX_BYTES;

    #[test]
    fn over_cap_refuses_with_warning_not_a_full_render() {
        let big = vec![b' '; (RENDER_MAX_BYTES + 1) as usize];
        let tm = Rc::new(ThemeManager::new(
            PeekThemeName::default(),
            StyleMode::Plain,
        ));
        let mut r = NotebookRenderer::new(InputSource::memory(big, "huge.ipynb"), Rc::clone(&tm));
        let lines = r
            .render(
                80,
                tm.peek_theme(),
                PeekThemeName::default(),
                StyleMode::Plain,
            )
            .unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("render cap"), "got: {:?}", lines[0]);
        assert_eq!(r.take_warnings().len(), 1);
    }

    #[test]
    fn image_notes_use_listing_names_in_order() {
        let nb = Notebook::parse(
            r#"{
              "cells": [
                {"cell_type":"code","source":["a"],
                 "outputs":[{"output_type":"display_data","data":{"image/png":"AAA="}}]},
                {"cell_type":"code","source":["b"],
                 "outputs":[{"output_type":"display_data","data":{"image/jpeg":"AAA="}}]}
              ],
              "metadata":{"kernelspec":{"language":"python"}},
              "nbformat":4,"nbformat_minor":5
            }"#,
        )
        .expect("parses");
        let md = to_markdown(&nb);
        // Inline notes carry the same names the Blocks listing assigns.
        assert!(md.contains("🖼\u{00a0} *image-1.png*"), "got: {md}");
        assert!(md.contains("🖼\u{00a0} *image-2.jpg*"), "got: {md}");
    }

    #[test]
    fn push_fence_grows_past_backtick_runs() {
        // No backticks: default 3-backtick fence.
        let mut md = String::new();
        push_fence(&mut md, "plain", "py");
        assert!(md.starts_with("```py\n"), "got: {md}");
        assert!(md.ends_with("```\n"), "got: {md}");

        // Body holds a run of 3 backticks — fence must grow to 4 so the
        // run cannot close the block.
        let mut md = String::new();
        push_fence(&mut md, "a```b", "");
        assert!(md.starts_with("````\n"), "got: {md}");
        assert!(md.ends_with("````\n"), "got: {md}");

        // Longest run wins: 5 backticks inside → 6-backtick fence.
        let mut md = String::new();
        push_fence(&mut md, "x`````y```z", "");
        assert!(md.starts_with("``````\n"), "got: {md}");
        assert!(md.ends_with("``````\n"), "got: {md}");
    }
}
