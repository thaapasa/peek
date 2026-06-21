//! HTML → ANSI-styled lines via `html2text`.
//!
//! Used by the standalone HTML mode and by the EPUB read mode (which
//! renders one XHTML chapter at a time through this same path). Lives
//! in its own file so neither view has to know about the other.

use std::io::Cursor;

use anyhow::Result;
use html2text::render::RichAnnotation;
use syntect::highlighting::Color;

use peek_io::sanitize_terminal_controls;
use peek_theme::{Attr, StyleMode};

/// Drive `html2text` to ANSI-styled lines. In `Plain` mode emits no
/// escapes; otherwise wraps annotated spans in SGR sequences via
/// `StyleMode` (so 256 / 16 / grayscale modes quantize HTML colors,
/// rather than leaking 24-bit escapes through the html2text path).
/// The `use_doc_css` builder pulls in inline `style="..."` and
/// `<style>` rules so author-defined colors and `font-weight: bold`
/// survive into the rendered text.
pub(crate) fn render(bytes: &[u8], width: usize, style_mode: StyleMode) -> Result<Vec<String>> {
    if !style_mode.styled() {
        let s = html2text::config::plain()
            .use_doc_css()
            .string_from_read(Cursor::new(bytes), width)?;
        // Plain mode carries no peek escapes — sanitize the finished lines.
        return Ok(s
            .lines()
            .map(|l| sanitize_terminal_controls(l).into_owned())
            .collect());
    }
    let s = html2text::config::rich().use_doc_css().coloured(
        Cursor::new(bytes),
        width,
        |annotations, text| annotate(style_mode, annotations, text),
    )?;
    Ok(s.lines().map(String::from).collect())
}

/// Map `html2text` rich annotations to ANSI SGR sequences via the
/// active `StyleMode`. Inner annotations are applied first so outer
/// formatting wraps them; the closing sequence is built in reverse so
/// escapes nest cleanly.
fn annotate(mode: StyleMode, annotations: &[RichAnnotation], text: &str) -> String {
    // Strip control codes from the leaf text *before* SGR-wrapping, so a raw
    // ESC/OSC in an HTML text node can't ride out through peek's own escapes.
    let text = sanitize_terminal_controls(text);
    let mut prefix = String::new();
    let mut suffix = String::new();
    for ann in annotations {
        let (open, close) = match ann {
            RichAnnotation::Strong => attr(mode, Attr::Bold),
            RichAnnotation::Emphasis => attr(mode, Attr::Italic),
            RichAnnotation::Code => attr(mode, Attr::Dim),
            RichAnnotation::Strikeout => attr(mode, Attr::Strikeout),
            // Links and image alt text don't carry their own color
            // here — author CSS supplies it via `Colour` annotations
            // when present, otherwise the attribute alone signals the
            // role.
            RichAnnotation::Link(_) => attr(mode, Attr::Underline),
            RichAnnotation::Image(_) => attr(mode, Attr::Italic),
            RichAnnotation::Colour(c) => {
                if is_grayscale(c) {
                    continue;
                }
                (mode.fg_seq(into_color(c)), mode.reset_fg().to_string())
            }
            RichAnnotation::BgColour(c) => {
                if is_grayscale(c) {
                    continue;
                }
                (mode.bg_seq(into_color(c)), mode.reset_bg().to_string())
            }
            _ => continue,
        };
        if open.is_empty() {
            continue;
        }
        prefix.push_str(&open);
        suffix.insert_str(0, &close);
    }
    if prefix.is_empty() {
        return text.into_owned();
    }
    let mut out = String::with_capacity(prefix.len() + text.len() + suffix.len());
    out.push_str(&prefix);
    out.push_str(&text);
    out.push_str(&suffix);
    out
}

fn attr(mode: StyleMode, a: Attr) -> (String, String) {
    (
        mode.attr_open(a).to_string(),
        mode.attr_close(a).to_string(),
    )
}

fn into_color(c: &html2text::Colour) -> Color {
    Color {
        r: c.r,
        g: c.g,
        b: c.b,
        a: 255,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // OSC 52 clipboard write + OSC 0 title spoof embedded in HTML text. BEL
    // (`\x07`) is the OSC terminator; peek never emits it, so its survival
    // would prove the injected sequence reached the terminal intact.
    const EVIL: &[u8] = b"x\x1b]52;c;cHduZWQ=\x07<b>\x1b]0;PWNED\x07bold</b>";

    #[test]
    fn plain_mode_strips_injected_controls() {
        let out = render(EVIL, 80, StyleMode::Plain).unwrap().join("\n");
        assert!(!out.contains('\x1b'), "ESC must not survive: {out:?}");
        assert!(!out.contains('\x07'), "BEL must not survive: {out:?}");
    }

    #[test]
    fn styled_mode_strips_injected_controls() {
        // TrueColor wraps spans in peek's own SGR (which contains ESC), so
        // we can't assert ESC-free — but peek's escapes never use BEL, so a
        // surviving BEL means the OSC payload leaked through annotate().
        let out = render(EVIL, 80, StyleMode::TrueColor).unwrap().join("\n");
        assert!(!out.contains('\x07'), "BEL must not survive: {out:?}");
        assert!(out.contains("bold"), "text must still render: {out:?}");
    }
}
