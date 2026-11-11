//! Build-time manual preprocessing: parse `SUMMARY.md` into TOC items and
//! rewrite each chapter for the in-app copy. Shared by `build.rs` (via
//! `#[path]`) and this crate's unit tests, so it uses only std and
//! pulldown-cmark. Errors are `String`s because `build.rs` only prints them.
//!
//! Chapter rewrites (authoring rules in `manual/README.md`):
//! - images are dropped (they are screenshots of peek itself);
//! - relative links keep their text but lose the target, so the renderer
//!   doesn't append a ` (./viewer/themes.md)` suffix;
//! - `<!-- peek: … -->` is unwrapped into Markdown;
//! - `<!-- peek-skip -->` … `<!-- /peek-skip -->` is removed.
//!
//! Edits are byte-range splices on the source, so the in-app source view
//! still shows the authored Markdown everywhere else.

use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Same extensions the Markdown renderer enables, so both parse the same
/// structure.
fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
}

#[derive(Debug, PartialEq, Eq)]
pub enum SummaryItem {
    /// `# File Types` part heading.
    Part(String),
    /// A chapter link. `depth` is the list nesting level (0 for top-level
    /// items and for prefix chapters outside any list).
    Chapter {
        title: String,
        path: String,
        depth: usize,
    },
}

/// Parse an mdbook `SUMMARY.md`. The first heading before any chapter is
/// the book title and is skipped. Draft chapters (empty link) and
/// separators are skipped too.
pub fn parse_summary(src: &str) -> Result<Vec<SummaryItem>, String> {
    let mut items = Vec::new();
    let mut list_depth = 0usize;
    let mut text: Option<String> = None;
    let mut link: Option<String> = None;
    let mut title_seen = false;

    for event in Parser::new_ext(src, options()) {
        match event {
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth -= 1,
            Event::Start(Tag::Heading { .. }) => text = Some(String::new()),
            Event::End(TagEnd::Heading(level)) => {
                let heading = text.take().unwrap_or_default();
                if !title_seen && items.is_empty() {
                    title_seen = true;
                } else if level == HeadingLevel::H1 {
                    items.push(SummaryItem::Part(heading.trim().to_string()));
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                link = Some(dest_url.to_string());
                text = Some(String::new());
            }
            Event::End(TagEnd::Link) => {
                let title = text.take().unwrap_or_default();
                let dest = link.take().unwrap_or_default();
                if dest.is_empty() {
                    continue;
                }
                items.push(SummaryItem::Chapter {
                    title: title.trim().to_string(),
                    path: normalize_path(&dest),
                    depth: list_depth.saturating_sub(1),
                });
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(buf) = text.as_mut() {
                    buf.push_str(&t);
                }
            }
            _ => {}
        }
    }
    Ok(items)
}

/// `./viewer/themes.md` → `viewer/themes.md`.
pub fn normalize_path(p: &str) -> String {
    p.trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

const SKIP_OPEN: &str = "<!-- peek-skip -->";
const SKIP_CLOSE: &str = "<!-- /peek-skip -->";
const PEEK_OPEN: &str = "<!-- peek:";
const MARKER_PREFIX: &str = "<!-- peek";
const MARKER_PREFIX_CLOSE: &str = "<!-- /peek";

/// Rewrite one chapter for the in-app copy. See the module header.
pub fn preprocess(src: &str) -> Result<String, String> {
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut skip_start: Option<usize> = None;

    for (event, range) in Parser::new_ext(src, options()).into_offset_iter() {
        match event {
            Event::Start(Tag::HtmlBlock) => {
                let block = src[range.clone()].trim();
                if block == SKIP_OPEN {
                    if skip_start.is_some() {
                        return Err("nested <!-- peek-skip -->".into());
                    }
                    skip_start = Some(range.start);
                } else if block == SKIP_CLOSE {
                    let start = skip_start
                        .take()
                        .ok_or("<!-- /peek-skip --> without an opening marker")?;
                    edits.push((start..range.end, String::new()));
                } else if let Some(rest) = block.strip_prefix(PEEK_OPEN) {
                    let body = rest
                        .strip_suffix("-->")
                        .ok_or("<!-- peek: … --> must end its block with -->")?
                        .trim();
                    if body.is_empty() {
                        return Err("empty <!-- peek: --> comment".into());
                    }
                    let mut body = preprocess(body)?;
                    body.push('\n');
                    // Keep the unwrapped body its own block if the authored
                    // text runs straight on after the comment.
                    if src[range.end..].chars().next().is_some_and(|c| c != '\n') {
                        body.push('\n');
                    }
                    edits.push((range, body));
                } else if block.starts_with(MARKER_PREFIX) || block.starts_with(MARKER_PREFIX_CLOSE)
                {
                    let first = block.lines().next().unwrap_or(block);
                    return Err(format!("unknown peek marker: {first}"));
                }
            }
            Event::Start(Tag::Image { .. }) => edits.push((range, String::new())),
            Event::Start(Tag::Link { dest_url, .. }) if is_relative(&dest_url) => {
                let authored = &src[range.clone()];
                // Inline links only: `[text](target)`. Reference-style
                // links have no `](` and are left as authored.
                if authored.ends_with(')')
                    && let Some(close) = authored.rfind("](")
                {
                    edits.push((range.clone(), format!("{}]()", &authored[..close])));
                }
            }
            _ => {}
        }
    }
    if skip_start.is_some() {
        return Err("unclosed <!-- peek-skip -->".into());
    }
    Ok(apply(src, edits))
}

/// Relative targets: other chapters, `#anchors`, image paths. Anything with
/// a scheme (`https:`, `mailto:`) is external.
fn is_relative(dest: &str) -> bool {
    !dest.is_empty() && !dest.contains("://") && !dest.starts_with("mailto:")
}

/// Splice `edits` into `src`. An edit nested inside an earlier one (an
/// image inside a `peek-skip` region) is dropped with it.
fn apply(src: &str, mut edits: Vec<(Range<usize>, String)>) -> String {
    edits.sort_by(|a, b| a.0.start.cmp(&b.0.start).then(b.0.end.cmp(&a.0.end)));
    let mut out = String::with_capacity(src.len());
    let mut cursor = 0;
    for (range, replacement) in edits {
        if range.start < cursor {
            continue;
        }
        out.push_str(&src[cursor..range.start]);
        out.push_str(&replacement);
        cursor = range.end;
    }
    out.push_str(&src[cursor..]);
    out
}
