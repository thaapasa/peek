//! Event walker. Holds the per-block accumulator and dispatches each
//! `pulldown-cmark` event to its handler. Output lines are emitted into
//! `out` as blocks close.

use pulldown_cmark::{Event, Tag, TagEnd};

use crate::theme::{PeekTheme, StyleMode};

use super::wrap::wrap_with_prefix;

/// Per-block accumulator. Only paragraph + heading text are wired in
/// the skeleton; richer block types arrive in follow-up commits.
pub(super) struct Walker<'a> {
    out: Vec<String>,
    /// Accumulated inline content for the current block (paragraph,
    /// heading, list item, etc.). Drained when the block closes.
    pending: String,
    /// Render width (terminal columns). Used as the wrap budget.
    width: usize,
    #[allow(dead_code)]
    theme: &'a PeekTheme,
    #[allow(dead_code)]
    style_mode: StyleMode,
    /// `true` between Paragraph / Heading start/end. Inline events
    /// outside an open block (e.g. a stray Text between blocks) are
    /// dropped — pulldown-cmark normally wraps them in a Paragraph.
    in_block: bool,
}

impl<'a> Walker<'a> {
    pub(super) fn new(width: usize, theme: &'a PeekTheme, style_mode: StyleMode) -> Self {
        Self {
            out: Vec::new(),
            pending: String::new(),
            width,
            theme,
            style_mode,
            in_block: false,
        }
    }

    pub(super) fn event(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(Tag::Paragraph) | Event::Start(Tag::Heading { .. }) => {
                self.in_block = true;
                self.pending.clear();
            }
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::Heading(_)) => {
                self.flush_block();
            }
            Event::Text(text) => {
                if self.in_block {
                    self.pending.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if self.in_block {
                    self.pending.push(' ');
                }
            }
            Event::Code(code) => {
                if self.in_block {
                    self.pending.push_str(&code);
                }
            }
            _ => {}
        }
    }

    fn flush_block(&mut self) {
        self.in_block = false;
        let body = std::mem::take(&mut self.pending);
        if body.is_empty() {
            return;
        }
        // Blank-line separator between blocks. Skip the leading one so
        // the document doesn't start with an empty row.
        if !self.out.is_empty() {
            self.out.push(String::new());
        }
        for line in wrap_with_prefix("", &body, self.width) {
            self.out.push(line);
        }
    }

    pub(super) fn finish(mut self) -> Vec<String> {
        if self.in_block {
            self.flush_block();
        }
        if self.out.is_empty() {
            self.out.push(String::new());
        }
        self.out
    }
}
