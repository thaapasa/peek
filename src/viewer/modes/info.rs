use anyhow::Result;

use super::{Mode, ModeId, RenderCtx, Window, slice_window};

pub(crate) struct InfoMode;

impl InfoMode {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Mode for InfoMode {
    fn id(&self) -> ModeId {
        ModeId::Info
    }

    fn label(&self) -> &str {
        "Info"
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let rendered = crate::info::render(ctx.file_info, ctx.peek_theme, ctx.render_opts);
        let full = wrap_info_lines(&rendered, ctx.term_cols);
        let total = full.len();
        let lines = slice_window(&full, scroll, rows);
        Ok(Window { lines, total })
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }
}

/// Wrap only the Info lines that actually overflow the content width.
///
/// Info fields are one terminal line each by construction, but a long
/// value (a decode-failure warning, a deep path) can exceed the width.
/// An over-wide line the terminal soft-wraps but the ScreenBuffer counts
/// as one row desyncs the redraw — the overflow tail stays painted after
/// the view changes. So those lines must be pre-split.
///
/// Lines that already fit pass through untouched: `push_field` builds its
/// aligned columns out of padding spaces, and a width-fit char wrapper
/// (`wrap_styled`, whitespace-preserving) is used for the rest — a
/// word wrapper would collapse those runs and destroy the alignment of
/// every field, not just the overflowing one.
fn wrap_info_lines(rendered: &[String], width: usize) -> Vec<String> {
    rendered
        .iter()
        .flat_map(|l| {
            if crate::viewer::ui::strip_ansi_width(l) > width {
                crate::viewer::ui::wrap_styled(l, width)
            } else {
                vec![l.clone()]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::wrap_info_lines;

    #[test]
    fn fitting_lines_keep_their_column_padding() {
        // push_field-style aligned line, well within width: must survive
        // byte-for-byte, indent and inter-column spaces intact.
        let lines = vec!["  Name        calendar.svg".to_string()];
        assert_eq!(wrap_info_lines(&lines, 80), lines);
    }

    #[test]
    fn only_overlong_lines_wrap_and_preserve_internal_space() {
        // 15 visible cols at width 10 → wraps; the first row keeps the
        // leading indent and the internal padding up to the cut (char
        // wrap, not word wrap which would collapse the spaces).
        let lines = vec!["  Name    value".to_string()];
        let out = wrap_info_lines(&lines, 10);
        assert!(out.len() >= 2, "overlong line must split: {out:?}");
        assert_eq!(out[0], "  Name    ");
    }

    #[test]
    fn exact_width_does_not_wrap() {
        let lines = vec!["1234567890".to_string()];
        assert_eq!(wrap_info_lines(&lines, 10), lines);
    }
}
