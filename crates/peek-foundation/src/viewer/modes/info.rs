use anyhow::Result;
use peek_theme::{PeekThemeName, StyleMode};

use super::{Mode, ModeId, RenderCtx, Window, slice_window};

#[derive(Default)]
pub struct InfoMode {
    /// Wrapped styled lines from the last render, kept so a scroll
    /// keystroke slices instead of re-theming every field. Same shape
    /// as `RenderedTextMode`'s cache.
    cache: Option<InfoCache>,
}

/// Everything that changes the rendered output. `warnings_len` covers
/// the one part of `FileInfo` that mutates mid-session — the session
/// layer appends (deduped) mode warnings, so the length moves whenever
/// the content does. `render_opts` is CLI-fixed and needs no key part.
#[derive(PartialEq, Eq)]
struct CacheKey {
    width: usize,
    theme_name: PeekThemeName,
    style_mode: StyleMode,
    warnings_len: usize,
}

struct InfoCache {
    key: CacheKey,
    lines: Vec<String>,
}

impl InfoMode {
    pub fn new() -> Self {
        Self::default()
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
        let key = CacheKey {
            width: ctx.term_cols,
            theme_name: ctx.theme_name,
            style_mode: ctx.peek_theme.style_mode,
            warnings_len: ctx.file_info.warnings.len(),
        };
        if self.cache.as_ref().is_none_or(|c| c.key != key) {
            let rendered = crate::info::render(ctx.file_info, ctx.peek_theme, ctx.render_opts);
            let lines = wrap_info_lines(&rendered, ctx.term_cols);
            self.cache = Some(InfoCache { key, lines });
        }
        let full = &self.cache.as_ref().expect("cache populated").lines;
        let total = full.len();
        let lines = slice_window(full, scroll, rows);
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
    use peek_theme::ThemeManager;

    use super::{wrap_info_lines, *};
    use crate::info::{FileInfo, NoExtras, RenderOptions};

    fn synthetic_file_info() -> FileInfo {
        FileInfo {
            file_name: "x".to_string(),
            path: "x".to_string(),
            size_bytes: 0,
            mimes: Vec::new(),
            warnings: Vec::new(),
            modified: None,
            created: None,
            permissions: None,
            compression: None,
            extras: Box::new(NoExtras),
        }
    }

    fn make_ctx<'a>(
        file_info: &'a FileInfo,
        peek_theme: &'a peek_theme::PeekTheme,
    ) -> RenderCtx<'a> {
        RenderCtx {
            file_info,
            theme_name: PeekThemeName::IdeaDark,
            peek_theme,
            render_opts: RenderOptions::default(),
            term_cols: 80,
            term_rows: 24,
        }
    }

    #[test]
    fn scroll_reuses_cached_lines_and_warnings_invalidate() {
        let tm = ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let peek_theme = tm.peek_theme().clone();
        let mut file_info = synthetic_file_info();
        let mut mode = InfoMode::new();

        let w1 = mode
            .render_window(&make_ctx(&file_info, &peek_theme), 0, 10)
            .unwrap();
        let cached_ptr = mode.cache.as_ref().unwrap().lines.as_ptr();

        // Scroll with nothing changed: the cache must survive untouched.
        let w2 = mode
            .render_window(&make_ctx(&file_info, &peek_theme), 1, 10)
            .unwrap();
        assert_eq!(w1.total, w2.total);
        assert_eq!(cached_ptr, mode.cache.as_ref().unwrap().lines.as_ptr());

        // A new warning must invalidate and surface in the output.
        file_info.warnings.push("late mode warning".to_string());
        let w3 = mode
            .render_window(&make_ctx(&file_info, &peek_theme), 0, 50)
            .unwrap();
        assert!(w3.total > w1.total, "warning line should appear");
        assert!(
            w3.lines.iter().any(|l| l.contains("late mode warning")),
            "warning text should render"
        );
    }

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
