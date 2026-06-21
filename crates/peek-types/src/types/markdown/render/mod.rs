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

mod table;
mod walker;
mod wrap;

use std::rc::Rc;

use anyhow::Result;
use pulldown_cmark::{Options, Parser};

use peek_io::sanitize_terminal_controls;
use peek_theme::{PeekTheme, PeekThemeName, StyleMode, ThemeManager};

/// Render `text` as styled markdown wrapped to `width` columns.
///
/// `width` is the terminal column count from `RenderedTextMode`. The
/// shared cache key handles invalidation so we don't need to track
/// width changes here. `theme_manager` + `theme_name` feed fenced
/// code blocks through syntect using the active syntax theme.
///
/// Also the shared entry for the notebook renderer (`render_markdown`),
/// so the control-char strip here covers both markdown and notebook text.
pub fn render(
    text: &str,
    width: usize,
    theme: &PeekTheme,
    style_mode: StyleMode,
    theme_manager: &Rc<ThemeManager>,
    theme_name: PeekThemeName,
) -> Result<Vec<String>> {
    // The walker paints text spans straight from the source, so strip
    // terminal-control sequences here — before parsing — or a hostile `.md`
    // / `.ipynb` could drive the terminal from its rendered view.
    let text = sanitize_terminal_controls(text);
    let (frontmatter, body) = split_frontmatter(&text);
    let parser = Parser::new_ext(body, gfm_options());
    let mut w = walker::Walker::new(width.max(20), theme, style_mode, theme_manager, theme_name);
    if let Some(fm) = frontmatter {
        w.emit_frontmatter(fm);
    }
    for ev in parser {
        w.event(ev);
    }
    Ok(w.finish())
}

/// Recognise a YAML (`---`) or TOML (`+++`) frontmatter block at the
/// top of the file. Returns the block (without its fence lines) plus
/// the body text the parser should see. CommonMark would otherwise
/// render the opening `---` as a horizontal rule and the key/value
/// lines as a setext heading + paragraph — useless and noisy.
fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let trimmed = text.strip_prefix('\u{feff}').unwrap_or(text);
    let fence = if trimmed.starts_with("---\n") || trimmed.starts_with("---\r\n") {
        "---"
    } else if trimmed.starts_with("+++\n") || trimmed.starts_with("+++\r\n") {
        "+++"
    } else {
        return (None, text);
    };
    let after_open = &trimmed[fence.len()..].trim_start_matches(['\r', '\n']);
    // Find the closing fence on its own line.
    let mut offset = 0;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == fence {
            let block = &after_open[..offset];
            let rest_start = offset + line.len();
            let rest = &after_open[rest_start..];
            return (Some(block), rest);
        }
        offset += line.len();
    }
    // Unclosed fence — leave the text alone.
    (None, text)
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
    use peek_theme::{PeekThemeName, ThemeManager, strip_ansi};
    use std::rc::Rc;

    fn render_plain(md: &str) -> Vec<String> {
        let tm = Rc::new(ThemeManager::new(
            PeekThemeName::default(),
            StyleMode::Plain,
        ));
        let theme = tm.peek_theme().clone();
        render(
            md,
            80,
            &theme,
            StyleMode::Plain,
            &tm,
            PeekThemeName::default(),
        )
        .unwrap()
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

    #[test]
    fn bullet_list_emits_marker_per_item() {
        let lines = render_plain("- one\n- two\n- three\n");
        let items: Vec<&String> = lines.iter().filter(|l| l.contains("•")).collect();
        assert_eq!(items.len(), 3, "expected three bullet rows, got {lines:?}");
    }

    #[test]
    fn ordered_list_numbers_from_start() {
        let lines = render_plain("3. third\n4. fourth\n");
        assert!(lines.iter().any(|l| l.contains("3. third")));
        assert!(lines.iter().any(|l| l.contains("4. fourth")));
    }

    #[test]
    fn blockquote_renders_with_rail() {
        let lines = render_plain("> quoted\n");
        assert!(
            lines
                .iter()
                .any(|l| l.contains("▍") && l.contains("quoted")),
            "expected rail + content, got {lines:?}"
        );
    }

    #[test]
    fn horizontal_rule_emits_dashes() {
        let lines = render_plain("text\n\n---\n\nmore\n");
        assert!(
            lines.iter().any(|l| l.contains("─")),
            "expected horizontal rule glyph, got {lines:?}"
        );
    }

    #[test]
    fn nested_list_indents_under_parent() {
        let lines = render_plain("- outer\n  - inner\n");
        let inner = lines.iter().find(|l| l.contains("inner")).unwrap();
        assert!(
            inner.starts_with("  "),
            "expected nested item indent, got {inner:?}"
        );
    }

    fn render_styled(md: &str) -> String {
        let tm = Rc::new(ThemeManager::new(
            PeekThemeName::default(),
            StyleMode::TrueColor,
        ));
        let theme = tm.peek_theme().clone();
        render(
            md,
            200,
            &theme,
            StyleMode::TrueColor,
            &tm,
            PeekThemeName::default(),
        )
        .unwrap()
        .join("\n")
    }

    #[test]
    fn link_url_appended_in_dim() {
        let out = render_styled("[label](https://example.com)\n");
        assert!(out.contains("label"));
        assert!(
            out.contains("(https://example.com)"),
            "expected URL suffix, got {out:?}"
        );
    }

    #[test]
    fn autolink_collapses_to_underlined_url_only() {
        let out = render_styled("<https://example.com>\n");
        // Underline open should appear; suffix should NOT duplicate the URL.
        let count = out.matches("https://example.com").count();
        assert_eq!(
            count, 1,
            "expected URL exactly once for autolink, got {out:?}"
        );
    }

    #[test]
    fn image_marked_with_alt_and_url() {
        let out = render_styled("![alt text](pic.png)\n");
        assert!(out.contains("[image:"));
        assert!(out.contains("alt text"));
        assert!(out.contains("(pic.png)"));
    }

    #[test]
    fn task_list_marker_replaces_bullet() {
        let lines = render_plain("- [x] done\n- [ ] todo\n- plain\n");
        let joined = lines.join("\n");
        assert!(
            joined.contains("✓ done"),
            "expected check + text in {joined:?}"
        );
        assert!(
            joined.contains("☐ todo"),
            "expected box + text in {joined:?}"
        );
        assert!(
            joined.contains("• plain"),
            "expected bullet for non-task in {joined:?}"
        );
    }

    #[test]
    fn inline_code_in_emphasis_keeps_outer_attr() {
        // A code span nested inside bold must not emit a universal reset
        // (`[0m`), which would clear the surrounding Bold state and leave
        // the trailing text unstyled.
        let out = render_styled("**bold `code` tail**\n");
        let bold_open = out.find("\x1b[1m").expect("expected bold open");
        let bold_close = out.find("\x1b[22m").expect("expected bold close");
        assert!(bold_close > bold_open, "bold close should follow open");
        let run = &out[bold_open..bold_close];
        assert!(
            !run.contains("\x1b[0m"),
            "inline code must not blow away outer bold via [0m, got {run:?}"
        );
        assert!(run.contains("tail"), "trailing text inside the bold run");
    }

    #[test]
    fn frontmatter_yaml_stripped_and_dimmed() {
        let out = render_styled("---\ntitle: x\n---\n# Heading\n\nbody\n");
        // YAML key should appear (dimmed) and `# Heading` should not
        // appear as plain text — it should be styled as a heading.
        assert!(out.contains("title: x"));
        // The `---` should NOT render as a horizontal rule, because we
        // stripped it before parsing.
        let stripped = strip_ansi(&out);
        assert!(
            !stripped.contains("──────────────"),
            "expected no HR row, got {stripped:?}"
        );
    }

    #[test]
    fn frontmatter_toml_stripped() {
        let lines = render_plain("+++\ntitle = \"x\"\n+++\n# Heading\n");
        let joined = lines.join("\n");
        assert!(joined.contains("title = \"x\""));
        assert!(joined.contains("Heading"));
    }

    #[test]
    fn footnote_reference_and_definition_render() {
        let lines = render_plain("Body[^a].\n\n[^a]: definition text\n");
        let joined = lines.join("\n");
        assert!(joined.contains("[^a]"), "expected ref marker in {joined:?}");
        assert!(
            joined.contains("[^a]:") && joined.contains("definition text"),
            "expected def header + body in {joined:?}"
        );
    }

    #[test]
    fn table_renders_box_drawing_with_header_separator() {
        let out = render_styled("| a | b |\n|---|---|\n| 1 | 2 |\n");
        // Top + head-sep + bottom borders use these corner glyphs.
        assert!(out.contains("┌"), "expected top border in {out:?}");
        assert!(out.contains("├"), "expected head separator in {out:?}");
        assert!(out.contains("└"), "expected bottom border in {out:?}");
        assert!(out.contains("│"), "expected vertical bars in {out:?}");
    }

    #[test]
    fn fenced_code_block_with_lang_emits_syntect_colors() {
        let out = render_styled("```rust\nfn main() {}\n```\n");
        // syntect emits truecolor escapes — a 38;2 fg sequence implies
        // the highlighter ran rather than the plain dim fallback.
        assert!(out.contains("38;2"), "expected syntect colors in {out:?}");
    }

    #[test]
    fn fenced_code_block_no_lang_paints_foreground_on_surface() {
        let out = render_styled("```\nbare text\n```\n");
        // No syntect highlighter, so the body is painted at full
        // foreground (38;2 truecolor) on the surface-tint fill — readable
        // light text, not the old dim ([2m) treatment.
        assert!(out.contains("38;2"), "expected foreground paint in {out:?}");
        assert!(!out.contains("\x1b[2m"), "should not be dim in {out:?}");
    }

    #[test]
    fn inline_code_in_blockquote_restores_surface_bg() {
        // Inline code closes with a bare bg reset (`[49m`). Inside a
        // blockquote the surface-fill post-pass must re-arm the surface bg
        // right after it, or the card drops to the terminal default for the
        // rest of the line. Assert every `[49m` is immediately followed by a
        // bg-open (`[48;2;`) so the fill never goes bare mid-line.
        let out = render_styled("> Lead `code` trail\n");
        for (i, _) in out.match_indices("\x1b[49m") {
            let after = &out[i + "\x1b[49m".len()..];
            assert!(
                after.starts_with("\x1b[48;2;"),
                "bg reset not re-armed with surface fill in {out:?}"
            );
        }
        // Sanity: the line really did contain a code span bg reset.
        assert!(out.contains("\x1b[49m"), "expected a bg reset in {out:?}");
    }

    #[test]
    fn emphasis_and_strong_emit_sgr() {
        let out = render_styled("Plain *em* **st** done.\n");
        // Italic = SGR 3; Bold = SGR 1.
        assert!(out.contains("\x1b[3m"), "expected italic open in {out:?}");
        assert!(out.contains("\x1b[1m"), "expected bold open in {out:?}");
    }
}
