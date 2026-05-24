//! Event walker. Tracks the open container stack (lists, blockquotes)
//! plus the current leaf block (paragraph, heading) and emits each
//! block through the wrap helper with the right prefix chain.
//!
//! The split is: containers add prefixes (list marker / indent,
//! blockquote rail) to every line a leaf inside them emits; the leaf
//! produces the body text. A list item's first emitted line consumes
//! the bullet / number; continuation lines fall back to whitespace of
//! matching width so wrapped text and nested blocks line up.

use std::rc::Rc;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, LinkType, Tag, TagEnd};

use crate::theme::{Attr, PeekTheme, PeekThemeName, StyleMode, ThemeManager};
use crate::viewer::highlight_lines;

use super::table;
use super::wrap::{display_width, wrap_with_prefix};

pub(super) struct Walker<'a> {
    out: Vec<String>,
    pending: String,
    width: usize,
    theme: &'a PeekTheme,
    style_mode: StyleMode,
    theme_manager: &'a Rc<ThemeManager>,
    theme_name: PeekThemeName,
    leaf: Option<Leaf>,
    /// Inline-span stack: each entry is the URL destination of an open
    /// link or image. End(Link/Image) pops the entry and appends a
    /// ` (url)` suffix in muted style when the URL is informative.
    inline_targets: Vec<InlineTarget>,
    /// `true` when the current leaf was opened implicitly (a tight
    /// list item's bare Text triggered it), `false` for an explicit
    /// Paragraph / Heading event. Used to decide whether a nested
    /// container should get a blank separator after the parent flush —
    /// tight items flow into their child list with no gap; loose items
    /// (real Paragraph wrappers) keep the gap.
    leaf_implicit: bool,
    containers: Vec<Container>,
    /// Set after an Item closes so the next implicit leaf opening
    /// (the next item's tight body) skips the inter-block blank line.
    /// Loose lists naturally space themselves via explicit Paragraph
    /// events outside the close-item suppress window.
    suppress_next_blank: bool,
    /// Some while a Table is open. Cells route inline events into the
    /// builder's current_cell buffer; End(Table) renders the result.
    table: Option<TableBuilder>,
    /// One-shot prefix consumed by the next emitted block's first
    /// line. Used by footnote definitions to inject the `[^label]: `
    /// header onto the inner Paragraph's first wrapped row.
    pending_first_line: Option<String>,
}

/// Accumulator for a table's parsed structure: rows of styled cell
/// strings. Cells preserve inline SGR (bold, italic, links, etc.) so
/// the rendered table cells match the surrounding prose styling.
struct TableBuilder {
    alignments: Vec<Alignment>,
    head: Vec<Vec<String>>,
    body: Vec<Vec<String>>,
    current_row: Vec<String>,
    in_head: bool,
    in_cell: bool,
}

/// Currently-open leaf block. Only one leaf is open at a time.
enum Leaf {
    Paragraph,
    Heading(HeadingLevel),
    /// Fenced or indented code block. `lang` is the declared info
    /// string token (empty for indented blocks); body buffers the raw
    /// source verbatim — no inline events fire inside a code block.
    CodeBlock {
        lang: String,
    },
}

/// Active link / image — the URL string is held so we can append it
/// after the inline text closes. Autolinks (`<http://x>`) and
/// reference-style links share the same shape.
enum InlineTarget {
    Link { url: String },
    Image { url: String },
}

/// An open container — list, list item, or blockquote. Stacked so
/// nesting is just push/pop and the prefix chain is the concatenation.
enum Container {
    /// Active list. Contributes no prefix; only holds the ordered
    /// counter so each item can mint the next marker.
    List {
        ordered: Option<u64>,
    },
    /// List item. `marker` is the first-line bullet / number, padded
    /// to its display width on continuation rows.
    Item {
        marker: String,
        marker_consumed: bool,
    },
    Blockquote,
}

impl<'a> Walker<'a> {
    pub(super) fn new(
        width: usize,
        theme: &'a PeekTheme,
        style_mode: StyleMode,
        theme_manager: &'a Rc<ThemeManager>,
        theme_name: PeekThemeName,
    ) -> Self {
        Self {
            out: Vec::new(),
            pending: String::new(),
            width,
            theme,
            style_mode,
            theme_manager,
            theme_name,
            leaf: None,
            inline_targets: Vec::new(),
            leaf_implicit: false,
            containers: Vec::new(),
            suppress_next_blank: false,
            table: None,
            pending_first_line: None,
        }
    }

    pub(super) fn event(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(Tag::Paragraph) => self.open_leaf(Leaf::Paragraph),
            Event::End(TagEnd::Paragraph) => self.close_leaf(),

            Event::Start(Tag::Heading { level, .. }) => self.open_leaf(Leaf::Heading(level)),
            Event::End(TagEnd::Heading(_)) => self.close_leaf(),

            Event::Start(Tag::List(start)) => {
                self.flush_open_leaf();
                self.push_list(start);
            }
            Event::End(TagEnd::List(_)) => self.pop_list(),

            Event::Start(Tag::Item) => {
                self.flush_open_leaf();
                self.open_item();
            }
            Event::End(TagEnd::Item) => self.close_item(),

            Event::Start(Tag::BlockQuote(_)) => {
                self.flush_open_leaf();
                // Insert the inter-block blank with the pre-push prefix
                // (rail-free), then push the BQ container and suppress
                // the first inner leaf's blank — otherwise the BQ would
                // open with a lone rail row, then another rail row of
                // content, doubling the visual gap.
                self.maybe_blank_separator();
                self.containers.push(Container::Blockquote);
                self.suppress_next_blank = true;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.containers.pop();
                self.suppress_next_blank = false;
            }

            Event::Start(Tag::Table(alignments)) => {
                self.flush_open_leaf();
                self.maybe_blank_separator();
                self.table = Some(TableBuilder {
                    alignments,
                    head: Vec::new(),
                    body: Vec::new(),
                    current_row: Vec::new(),
                    in_head: false,
                    in_cell: false,
                });
            }
            Event::End(TagEnd::Table) => self.finish_table(),
            Event::Start(Tag::TableHead) => {
                if let Some(t) = &mut self.table {
                    t.in_head = true;
                    t.current_row.clear();
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.current_row);
                    if !row.is_empty() {
                        t.head.push(row);
                    }
                    t.in_head = false;
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(t) = &mut self.table {
                    t.current_row.clear();
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.current_row);
                    t.body.push(row);
                }
            }
            Event::Start(Tag::TableCell) => {
                // Reuse the existing inline accumulator (`pending`) by
                // opening a transient leaf; End(TableCell) drains it
                // into the row instead of out.
                if self.table.is_some() {
                    self.leaf = Some(Leaf::Paragraph);
                    self.leaf_implicit = false;
                    self.pending.clear();
                    if let Some(t) = &mut self.table {
                        t.in_cell = true;
                    }
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(t) = &mut self.table {
                    let cell = std::mem::take(&mut self.pending);
                    t.current_row.push(cell);
                    t.in_cell = false;
                    self.leaf = None;
                }
            }

            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.into_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.flush_open_leaf();
                self.maybe_blank_separator();
                self.leaf = Some(Leaf::CodeBlock { lang });
                self.leaf_implicit = false;
                self.pending.clear();
            }
            Event::End(TagEnd::CodeBlock) => self.close_leaf(),

            Event::Rule => self.emit_rule(),

            Event::TaskListMarker(checked) => self.apply_task_marker(checked),

            Event::FootnoteReference(label) => {
                self.ensure_leaf_for_inline();
                if self.leaf.is_some() {
                    let txt = format!("[^{label}]");
                    self.pending.push_str(&self.theme.paint_accent(&txt));
                }
            }

            Event::Start(Tag::FootnoteDefinition(label)) => {
                // pulldown wraps the definition content in its own
                // Paragraph; let that flow through normally and inject
                // the label as a one-shot first-line prefix.
                self.flush_open_leaf();
                self.maybe_blank_separator();
                let label = format!("[^{label}]: ");
                self.pending_first_line = Some(self.theme.paint_accent(&label));
                // The inner Paragraph's open_leaf would otherwise add
                // a second blank on top of the one we just emitted.
                self.suppress_next_blank = true;
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                self.pending_first_line = None;
            }

            Event::Start(Tag::Emphasis) => self.push_inline_attr(Attr::Italic),
            Event::End(TagEnd::Emphasis) => self.pop_inline_attr(Attr::Italic),
            Event::Start(Tag::Strong) => self.push_inline_attr(Attr::Bold),
            Event::End(TagEnd::Strong) => self.pop_inline_attr(Attr::Bold),
            Event::Start(Tag::Strikethrough) => self.push_inline_attr(Attr::Strikeout),
            Event::End(TagEnd::Strikethrough) => self.pop_inline_attr(Attr::Strikeout),

            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => self.start_link(link_type, dest_url.into_string()),
            Event::End(TagEnd::Link) => self.end_link(),

            Event::Start(Tag::Image { dest_url, .. }) => {
                self.start_image(dest_url.into_string());
            }
            Event::End(TagEnd::Image) => self.end_image(),

            Event::Text(text) => {
                self.ensure_leaf_for_inline();
                if self.leaf.is_some() {
                    self.pending.push_str(&text);
                }
            }
            Event::Code(code) => {
                // Inline code only — code-block bodies arrive as Text
                // events, not Code.
                self.ensure_leaf_for_inline();
                if self.leaf.is_some() {
                    self.pending.push_str(self.style_mode.attr_open(Attr::Dim));
                    self.pending.push_str(&code);
                    self.pending.push_str(self.style_mode.attr_close(Attr::Dim));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if self.leaf.is_some() {
                    self.pending.push(' ');
                }
            }
            _ => {}
        }
    }

    pub(super) fn finish(mut self) -> Vec<String> {
        if self.leaf.is_some() {
            self.close_leaf();
        }
        if self.out.is_empty() {
            self.out.push(String::new());
        }
        self.out
    }

    fn open_leaf(&mut self, leaf: Leaf) {
        self.maybe_blank_separator();
        self.leaf = Some(leaf);
        self.leaf_implicit = false;
        self.pending.clear();
    }

    fn close_leaf(&mut self) {
        let Some(leaf) = self.leaf.take() else {
            return;
        };
        let body = std::mem::take(&mut self.pending);
        match &leaf {
            Leaf::Paragraph => self.emit_lines(&body),
            Leaf::Heading(level) => {
                self.emit_lines(&style_heading(&body, *level, self.theme));
                if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) {
                    self.emit_heading_underline(*level);
                }
            }
            Leaf::CodeBlock { lang } => self.emit_code_block(&body, lang),
        }
    }

    fn push_list(&mut self, start: Option<u64>) {
        self.containers.push(Container::List { ordered: start });
    }

    fn pop_list(&mut self) {
        // Should always pop a List; ignore mismatch defensively.
        if let Some(Container::List { .. }) = self.containers.last() {
            self.containers.pop();
        }
        // Once we exit the list, the next block (paragraph, second
        // list, blockquote) wants the normal blank-line separator —
        // the suppress flag belongs to between-item spacing only.
        self.suppress_next_blank = false;
    }

    fn open_item(&mut self) {
        let marker = match self.containers.iter_mut().rev().find_map(|c| match c {
            Container::List { ordered } => Some(ordered),
            _ => None,
        }) {
            Some(ord @ Some(_)) => {
                let n = ord.unwrap();
                *ord = Some(n + 1);
                format!("{n}. ")
            }
            _ => "• ".to_string(),
        };
        self.containers.push(Container::Item {
            marker,
            marker_consumed: false,
        });
    }

    fn close_item(&mut self) {
        // Tight items carry inline Text without a Paragraph wrapper.
        // If we implicitly opened one in `ensure_leaf_for_inline`, flush
        // it now so the item's content lands before the marker pops.
        if self.leaf.is_some() {
            self.close_leaf();
        }
        if let Some(Container::Item { .. }) = self.containers.last() {
            self.containers.pop();
        }
        self.suppress_next_blank = true;
    }

    fn push_inline_attr(&mut self, attr: Attr) {
        self.ensure_leaf_for_inline();
        if self.leaf.is_some() {
            self.pending.push_str(self.style_mode.attr_open(attr));
        }
    }

    fn pop_inline_attr(&mut self, attr: Attr) {
        if self.leaf.is_some() {
            self.pending.push_str(self.style_mode.attr_close(attr));
        }
    }

    fn start_link(&mut self, link_type: LinkType, url: String) {
        self.ensure_leaf_for_inline();
        if self.leaf.is_none() {
            return;
        }
        // Autolinks (`<http://x>`) print the URL twice if we underline
        // the text *and* append it after — collapse to underline only.
        let suppress_suffix =
            matches!(link_type, LinkType::Autolink | LinkType::Email) || url.is_empty();
        let url = if suppress_suffix { String::new() } else { url };
        self.pending
            .push_str(self.style_mode.attr_open(Attr::Underline));
        self.inline_targets.push(InlineTarget::Link { url });
    }

    fn end_link(&mut self) {
        if self.leaf.is_some() {
            self.pending
                .push_str(self.style_mode.attr_close(Attr::Underline));
        }
        if let Some(InlineTarget::Link { url }) = self.inline_targets.pop()
            && !url.is_empty()
            && self.leaf.is_some()
        {
            self.append_target_suffix(&url);
        }
    }

    fn start_image(&mut self, url: String) {
        self.ensure_leaf_for_inline();
        if self.leaf.is_none() {
            return;
        }
        // The alt-text events flow inside Start/End image as Text.
        // Mark the alt with `[image: ` … `]` so it's recognisable in a
        // terminal-only render.
        self.pending.push_str(&self.theme.paint_muted("[image: "));
        self.inline_targets.push(InlineTarget::Image { url });
    }

    fn end_image(&mut self) {
        if self.leaf.is_some() {
            self.pending.push_str(&self.theme.paint_muted("]"));
        }
        if let Some(InlineTarget::Image { url }) = self.inline_targets.pop()
            && !url.is_empty()
            && self.leaf.is_some()
        {
            self.append_target_suffix(&url);
        }
    }

    fn append_target_suffix(&mut self, url: &str) {
        let suffix = format!(" ({url})");
        self.pending.push_str(&self.theme.paint_muted(&suffix));
    }

    /// Flush an open leaf — emits its accumulated body so a new
    /// container (nested list, blockquote) opens cleanly without
    /// stealing text from the current item's body. An implicit-leaf
    /// flush also suppresses the next blank: tight items flow into
    /// their nested container with no gap.
    fn flush_open_leaf(&mut self) {
        if self.leaf.is_some() {
            let was_implicit = self.leaf_implicit;
            self.close_leaf();
            if was_implicit {
                self.suppress_next_blank = true;
            }
        }
    }

    /// Tight list items emit inline events directly at the Item level
    /// (no Paragraph wrapper). Open an implicit Paragraph so the text
    /// has somewhere to land.
    fn ensure_leaf_for_inline(&mut self) {
        if self.leaf.is_some() {
            return;
        }
        if matches!(self.containers.last(), Some(Container::Item { .. })) {
            self.open_leaf(Leaf::Paragraph);
            self.leaf_implicit = true;
        }
    }

    /// Replace the current item's bullet with a task-list glyph
    /// (`☐ ` / `✓ `). pulldown emits the TaskListMarker event right
    /// after Start(Item) and before any inline content, so the marker
    /// hasn't been consumed yet.
    fn apply_task_marker(&mut self, checked: bool) {
        if let Some(Container::Item {
            marker,
            marker_consumed,
        }) = self.containers.last_mut()
            && !*marker_consumed
        {
            *marker = if checked {
                "✓ ".to_string()
            } else {
                "☐ ".to_string()
            };
        }
    }

    /// Emit the verbatim frontmatter block at the top of the document
    /// as a dim wrapped paragraph. Each source line becomes one output
    /// line — preserves YAML / TOML indentation. The next block's
    /// `open_leaf` handles the inter-block blank, so no separator row
    /// is pushed here.
    pub(super) fn emit_frontmatter(&mut self, fm: &str) {
        for line in fm.lines() {
            let dim = format!(
                "{}{}{}",
                self.style_mode.attr_open(Attr::Dim),
                line,
                self.style_mode.attr_close(Attr::Dim)
            );
            self.out.push(dim);
        }
    }

    /// Render the accumulated table into box-drawing rows and emit
    /// each with the current container prefix.
    fn finish_table(&mut self) {
        let Some(t) = self.table.take() else {
            return;
        };
        let prefix = self.continuation_prefix();
        let available = self.width.saturating_sub(display_width(&prefix)).max(8);
        let rows = table::render(&t.head, &t.body, &t.alignments, available, self.theme);
        for row in rows {
            self.out.push(format!("{prefix}{row}"));
        }
    }

    /// Render a fenced or indented code block. Tries to highlight via
    /// syntect when `lang` resolves; falls back to dim-styled plain
    /// lines on miss / failure. Each row gets the container chain
    /// prefix plus an `  ` indent so code stands apart from prose.
    fn emit_code_block(&mut self, body: &str, lang: &str) {
        let prefix = self.continuation_prefix();
        let indent = "  ";
        let highlighted: Option<Vec<String>> = (!lang.is_empty()
            && self.style_mode != StyleMode::Plain)
            .then(|| {
                highlight_lines(
                    body,
                    lang,
                    self.theme_manager,
                    self.theme_name,
                    self.style_mode,
                )
                .ok()
            })
            .flatten();

        let rows: Vec<String> = match highlighted {
            Some(lines) => lines,
            None => body
                .lines()
                .map(|l| {
                    format!(
                        "{}{}{}",
                        self.style_mode.attr_open(Attr::Dim),
                        l,
                        self.style_mode.attr_close(Attr::Dim)
                    )
                })
                .collect(),
        };

        for row in rows {
            self.out.push(format!("{prefix}{indent}{row}"));
        }
    }

    fn emit_rule(&mut self) {
        self.maybe_blank_separator();
        let prefix = self.continuation_prefix();
        let avail = self.width.saturating_sub(display_width(&prefix)).max(1);
        let rule = self.theme.paint_muted(&"─".repeat(avail));
        self.out.push(format!("{prefix}{rule}"));
    }

    fn emit_heading_underline(&mut self, level: HeadingLevel) {
        let prefix = self.continuation_prefix();
        let avail = self.width.saturating_sub(display_width(&prefix)).max(1);
        let ch = if matches!(level, HeadingLevel::H1) {
            "═"
        } else {
            "─"
        };
        let underline = self.theme.paint(&ch.repeat(avail), self.theme.heading);
        self.out.push(format!("{prefix}{underline}"));
    }

    /// Push a blank line between adjacent blocks. The blank still
    /// carries the container-chain prefix (e.g. blockquote rail) so
    /// nested structure stays visible across the gap. Suppressed once
    /// after each item close — tight list items render contiguously.
    fn maybe_blank_separator(&mut self) {
        if self.out.is_empty() {
            return;
        }
        if std::mem::take(&mut self.suppress_next_blank) {
            return;
        }
        let prefix = self.continuation_prefix();
        self.out.push(prefix.trim_end().to_string());
    }

    fn emit_lines(&mut self, body: &str) {
        if body.is_empty() && self.containers.is_empty() {
            return;
        }
        let first_prefix = self.first_line_prefix();
        let cont_prefix = self.continuation_prefix();
        let lines = wrap_with_prefix_split(&first_prefix, &cont_prefix, body, self.width);
        for line in lines {
            self.out.push(line);
        }
    }

    /// Prefix for the first emitted line of the next block — consumes
    /// the deepest list item's marker, if any.
    fn first_line_prefix(&mut self) -> String {
        let mut out = String::new();
        for c in &mut self.containers {
            match c {
                Container::List { .. } => {}
                Container::Blockquote => out.push_str(&blockquote_rail(self.theme)),
                Container::Item {
                    marker,
                    marker_consumed,
                } => {
                    if *marker_consumed {
                        out.push_str(&" ".repeat(display_width(marker)));
                    } else {
                        out.push_str(&self.theme.paint_accent(marker));
                        *marker_consumed = true;
                    }
                }
            }
        }
        if let Some(extra) = self.pending_first_line.take() {
            out.push_str(&extra);
        }
        out
    }

    /// Prefix for wrapped continuation rows + blank separators. Each
    /// list-item marker shows as whitespace of matching width.
    fn continuation_prefix(&self) -> String {
        let mut out = String::new();
        for c in &self.containers {
            match c {
                Container::List { .. } => {}
                Container::Blockquote => out.push_str(&blockquote_rail(self.theme)),
                Container::Item { marker, .. } => {
                    out.push_str(&" ".repeat(display_width(marker)));
                }
            }
        }
        out
    }
}

fn style_heading(text: &str, level: HeadingLevel, theme: &PeekTheme) -> String {
    let _ = level;
    theme.paint_heading(text)
}

fn blockquote_rail(theme: &PeekTheme) -> String {
    theme.paint_muted("▍ ")
}

/// Wrap a body with separate first-line + continuation prefixes. Built
/// on the shared `wrap_with_prefix` — the first chunk gets the leading
/// prefix, every later chunk gets the continuation form.
fn wrap_with_prefix_split(first: &str, cont: &str, body: &str, width: usize) -> Vec<String> {
    // Wrap once at the continuation width (the conservative budget),
    // then swap the first row's prefix back to the leading form. This
    // keeps the math simple at the cost of a slightly narrower first
    // row when the two prefixes differ in width — fine in practice,
    // since first/cont differ only by marker glyph width (≤ 4 cols).
    let mut rows = wrap_with_prefix(cont, body, width);
    if rows.is_empty() {
        rows.push(first.to_string());
        return rows;
    }
    let first_body = rows[0].strip_prefix(cont).unwrap_or(&rows[0]).to_string();
    rows[0] = format!("{first}{first_body}");
    rows
}
