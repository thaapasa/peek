//! Event walker. Tracks the open container stack (lists, blockquotes)
//! plus the current leaf block (paragraph, heading) and emits each
//! block through the wrap helper with the right prefix chain.
//!
//! The split is: containers add prefixes (list marker / indent,
//! blockquote rail) to every line a leaf inside them emits; the leaf
//! produces the body text. A list item's first emitted line consumes
//! the bullet / number; continuation lines fall back to whitespace of
//! matching width so wrapped text and nested blocks line up.

use pulldown_cmark::{Event, HeadingLevel, LinkType, Tag, TagEnd};

use crate::theme::{Attr, PeekTheme, StyleMode};

use super::wrap::{display_width, wrap_with_prefix};

pub(super) struct Walker<'a> {
    out: Vec<String>,
    pending: String,
    width: usize,
    theme: &'a PeekTheme,
    #[allow(dead_code)]
    style_mode: StyleMode,
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
}

/// Currently-open leaf block. Only one leaf is open at a time.
enum Leaf {
    Paragraph,
    Heading(HeadingLevel),
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
    pub(super) fn new(width: usize, theme: &'a PeekTheme, style_mode: StyleMode) -> Self {
        Self {
            out: Vec::new(),
            pending: String::new(),
            width,
            theme,
            style_mode,
            leaf: None,
            inline_targets: Vec::new(),
            leaf_implicit: false,
            containers: Vec::new(),
            suppress_next_blank: false,
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

            Event::Rule => self.emit_rule(),

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
        let body = match leaf {
            Leaf::Paragraph => body,
            Leaf::Heading(level) => style_heading(&body, level, self.theme),
        };
        self.emit_lines(&body);
        if let Leaf::Heading(level) = leaf
            && matches!(level, HeadingLevel::H1 | HeadingLevel::H2)
        {
            self.emit_heading_underline(level);
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
