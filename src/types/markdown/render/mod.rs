//! Markdown → width-wrapped, ANSI-styled lines.
//!
//! Drives `pulldown-cmark`'s event stream, accumulates inline content
//! per block, then emits styled output through `wrap`. Each block type
//! (paragraph, heading, list, blockquote, code, table) lives in its own
//! handler so the dispatch stays flat.
//!
//! Inline styles ride along as SGR open/close pairs inside the
//! accumulated text; `wrap` re-applies the active style after each cut
//! so styled spans survive a line break.

mod walker;
mod wrap;

use anyhow::Result;
use pulldown_cmark::{Options, Parser};

use crate::theme::{PeekTheme, StyleMode};

/// Render `text` as styled markdown wrapped to `width` columns.
///
/// `width` is the terminal column count from `RenderedTextMode`. The
/// shared cache key handles invalidation so we don't need to track
/// width changes here.
pub fn render(
    text: &str,
    width: usize,
    theme: &PeekTheme,
    style_mode: StyleMode,
) -> Result<Vec<String>> {
    let parser = Parser::new_ext(text, gfm_options());
    let mut w = walker::Walker::new(width.max(20), theme, style_mode);
    for ev in parser {
        w.event(ev);
    }
    Ok(w.finish())
}

/// CommonMark + GFM. Same flag set mdbook uses; gives tables /
/// strikethrough / task lists / footnotes without pulling extra crates.
fn gfm_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{PeekThemeName, ThemeManager};

    fn render_plain(md: &str) -> Vec<String> {
        let tm = ThemeManager::new(PeekThemeName::default(), StyleMode::Plain);
        let theme = tm.peek_theme().clone();
        render(md, 80, &theme, StyleMode::Plain).unwrap()
    }

    #[test]
    fn paragraph_text_emitted() {
        let lines = render_plain("hello world\n");
        assert!(
            lines.iter().any(|l| l.contains("hello world")),
            "expected paragraph text in output, got {lines:?}"
        );
    }

    #[test]
    fn blank_line_separates_paragraphs() {
        let lines = render_plain("first\n\nsecond\n");
        let first = lines.iter().position(|l| l.contains("first")).unwrap();
        let second = lines.iter().position(|l| l.contains("second")).unwrap();
        assert!(second > first + 1, "expected blank line between paragraphs");
    }
}
