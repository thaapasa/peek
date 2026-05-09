//! Rendered HTML view backed by `html2text`.
//!
//! Renders the whole document into wrapped, ANSI-styled lines on first
//! use and on terminal width changes. Rendering is whole-document
//! (html2text has no streaming API), so very large HTML may pause on
//! first render — typical pages are well under 1 MB and render
//! instantly.

use std::fmt::Write;
use std::io::Cursor;

use anyhow::Result;
use html2text::render::RichAnnotation;

use crate::input::InputSource;
use crate::theme::ColorMode;
use crate::viewer::modes::{Mode, ModeId, RenderCtx, Window, slice_window};

pub(crate) struct RenderedMode {
    source: InputSource,
    color_mode: ColorMode,
    /// Cached render keyed by the width it was produced for. `None`
    /// before the first render; replaced when `term_cols` changes.
    cache: Option<Cached>,
}

struct Cached {
    width: usize,
    lines: Vec<String>,
}

impl RenderedMode {
    pub(crate) fn new(source: InputSource, color_mode: ColorMode) -> Self {
        Self {
            source,
            color_mode,
            cache: None,
        }
    }

    fn ensure_rendered(&mut self, width: usize) -> Result<&[String]> {
        let needs_render = self
            .cache
            .as_ref()
            .map(|c| c.width != width)
            .unwrap_or(true);
        if needs_render {
            let bytes = self.source.read_bytes()?;
            let lines = render_html(&bytes, width.max(20), self.color_mode)?;
            self.cache = Some(Cached { width, lines });
        }
        Ok(&self.cache.as_ref().expect("cache populated").lines)
    }
}

impl Mode for RenderedMode {
    fn id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn label(&self) -> &str {
        "Rendered"
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let lines = self.ensure_rendered(ctx.term_cols)?;
        let total = lines.len();
        let win = slice_window(lines, scroll, rows);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache.as_ref().map(|c| c.lines.len())
    }
}

/// Drive `html2text` to ANSI-styled lines. In `Plain` color mode emits
/// no escapes; otherwise wraps annotated spans in SGR sequences. The
/// `use_doc_css` builder pulls in inline `style="..."` and `<style>`
/// rules so author-defined colors and `font-weight: bold` survive into
/// the rendered text.
fn render_html(bytes: &[u8], width: usize, color_mode: ColorMode) -> Result<Vec<String>> {
    let rendered = if matches!(color_mode, ColorMode::Plain) {
        html2text::config::plain()
            .use_doc_css()
            .string_from_read(Cursor::new(bytes), width)?
    } else {
        html2text::config::rich()
            .use_doc_css()
            .coloured(Cursor::new(bytes), width, annotate)?
    };
    Ok(rendered.lines().map(String::from).collect())
}

/// Map `html2text` rich annotations to ANSI SGR sequences. Inner
/// annotations are applied first so outer formatting wraps them; the
/// closing sequence is built in reverse so escapes nest cleanly.
fn annotate(annotations: &[RichAnnotation], text: &str) -> String {
    let mut prefix = String::new();
    let mut suffix = String::new();
    for ann in annotations {
        let (open, close) = match ann {
            RichAnnotation::Strong => ("\x1b[1m".to_string(), "\x1b[22m".to_string()),
            RichAnnotation::Emphasis => ("\x1b[3m".to_string(), "\x1b[23m".to_string()),
            RichAnnotation::Code => ("\x1b[2m".to_string(), "\x1b[22m".to_string()),
            RichAnnotation::Strikeout => ("\x1b[9m".to_string(), "\x1b[29m".to_string()),
            RichAnnotation::Link(_) => ("\x1b[4;94m".to_string(), "\x1b[24;39m".to_string()),
            RichAnnotation::Image(_) => ("\x1b[35m".to_string(), "\x1b[39m".to_string()),
            RichAnnotation::Colour(c) => {
                if is_grayscale(c) {
                    continue;
                }
                (sgr_rgb(c, true), "\x1b[39m".to_string())
            }
            RichAnnotation::BgColour(c) => {
                if is_grayscale(c) {
                    continue;
                }
                (sgr_rgb(c, false), "\x1b[49m".to_string())
            }
            _ => continue,
        };
        prefix.push_str(&open);
        suffix.insert_str(0, &close);
    }
    if prefix.is_empty() {
        text.to_string()
    } else {
        let mut out = String::with_capacity(prefix.len() + text.len() + suffix.len());
        out.push_str(&prefix);
        out.push_str(text);
        out.push_str(&suffix);
        out
    }
}

fn sgr_rgb(c: &html2text::Colour, fg: bool) -> String {
    let lead = if fg { 38 } else { 48 };
    let mut s = String::new();
    let _ = write!(&mut s, "\x1b[{lead};2;{};{};{}m", c.r, c.g, c.b);
    s
}

/// Treat near-grayscale colors as "body default" and skip them. Author
/// stylesheets typically set `color:#1f1f1f` (or similar) on `body`,
/// which then propagates to every span — emitting the escape would
/// fight the user's terminal foreground. Saturated accents
/// (`max - min` above the threshold) still render.
fn is_grayscale(c: &html2text::Colour) -> bool {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    max.saturating_sub(min) < 24
}
