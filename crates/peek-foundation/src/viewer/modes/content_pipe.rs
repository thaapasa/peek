//! Pipe-mode rendering for [`ContentMode`](super::content::ContentMode).
//!
//! Three independent branches — pretty whole-text write, raw stream
//! with highlighter, raw stream without highlighter — and a small
//! gutter prefix builder shared by the two raw paths. The
//! [`ContentMode`](super::content::ContentMode) trait impl is a thin
//! caller over [`render`].
//!
//! With a syntax token, every raw line (including the last) is
//! `\n`-terminated — pre-A1 contract: escape sequences are line-scoped
//! and the natural shape is per-line writes. Without a token, preserve
//! the source's trailing-newline status for byte-for-byte fidelity
//! (matches `cat`).

use std::rc::Rc;

use anyhow::Result;

use super::super::{LineStreamHighlighter, highlight_lines};
use super::RenderCtx;
use super::gutter::Gutter;
use crate::input::LineSource;
use crate::output::PrintOutput;
use crate::theme::ThemeManager;

#[allow(clippy::too_many_arguments)]
pub(super) fn render(
    ctx: &RenderCtx,
    out: &mut PrintOutput,
    pretty_text: Option<&str>,
    line_source: &LineSource,
    highlighter: Option<&mut LineStreamHighlighter>,
    syntax_token: Option<&str>,
    theme_manager: &Rc<ThemeManager>,
    gutter: &Gutter,
) -> Result<()> {
    let style_mode = ctx.peek_theme.style_mode;
    if let Some(pretty) = pretty_text {
        if let Some(token) = syntax_token {
            let mut lines =
                highlight_lines(pretty, token, theme_manager, ctx.theme_name, style_mode)?;
            let total = lines.len();
            gutter.apply(&mut lines, 0, total, ctx.peek_theme);
            for line in &lines {
                out.write_line(line)?;
            }
        } else if gutter.enabled() {
            let mut lines: Vec<String> = pretty.lines().map(String::from).collect();
            let total = lines.len();
            gutter.apply(&mut lines, 0, total, ctx.peek_theme);
            for line in &lines {
                out.write_line(line)?;
            }
        } else {
            out.write_str(pretty)?;
        }
        return Ok(());
    }

    let total = line_source.total_lines();
    let prefix = gutter.stream_prefixer(total, ctx.peek_theme);

    if let Some(hl) = highlighter {
        hl.reset(ctx.theme_name);
        for (idx, line) in line_source.iter_all().enumerate() {
            let line = line?;
            let escaped = hl.feed(&line, style_mode)?;
            if let Some(p) = prefix(idx + 1) {
                out.write_line(&format!("{p}{escaped}"))?;
            } else {
                out.write_line(&escaped)?;
            }
        }
    } else {
        let trailing_nl = line_source.ends_with_newline();
        for (idx, line) in line_source.iter_all().enumerate() {
            let line = line?;
            let is_last = idx + 1 == total;
            let body = if let Some(p) = prefix(idx + 1) {
                format!("{p}{line}")
            } else {
                line
            };
            if is_last && !trailing_nl {
                out.write_str(&body)?;
            } else {
                out.write_line(&body)?;
            }
        }
    }
    Ok(())
}
