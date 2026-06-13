//! `TextRenderer` for an email message: a themed header block followed
//! by the rendered body. HTML bodies route through the shared html2text
//! driver (`crate::types::html::render`) so an email and an `.html` file
//! read identically; plain-text bodies are word-wrapped to the viewport.
//!
//! Rendering re-reads + re-parses the source on each call (like
//! `HtmlRenderer`); the generic [`RenderedTextMode`] caches the wrapped
//! result per `(width, style_mode, theme)`, so only resizes / theme
//! cycles pay the parse again.

use anyhow::Result;

use crate::input::InputSource;
use crate::theme::{PeekTheme, PeekThemeName, StyleMode, display_width};
use crate::types::html;
use crate::viewer::modes::{ModeId, TextRenderer};
use crate::viewer::ui::wrap_styled_words;

use super::message::{self, Body};

pub(crate) struct EmailRenderer {
    source: InputSource,
}

impl EmailRenderer {
    pub(crate) fn new(source: InputSource) -> Self {
        Self { source }
    }
}

impl TextRenderer for EmailRenderer {
    fn label(&self) -> &'static str {
        "Message"
    }

    fn mode_id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn render(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        _theme_name: PeekThemeName,
        style_mode: StyleMode,
    ) -> Result<Vec<String>> {
        let width = width.max(20);
        let bytes = self
            .source
            .read_bytes(crate::input::limits::Budget::Sidecar("email"))?;
        let Some(email) = message::parse(&bytes) else {
            return Ok(vec![theme.paint_muted("[unparseable email]")]);
        };

        let mut lines = Vec::new();
        push_header(&mut lines, "From", email.from.as_deref(), theme, width);
        push_header(&mut lines, "To", email.to.as_deref(), theme, width);
        push_header(&mut lines, "Cc", email.cc.as_deref(), theme, width);
        push_header(&mut lines, "Date", email.date.as_deref(), theme, width);
        push_header(
            &mut lines,
            "Subject",
            email.subject.as_deref(),
            theme,
            width,
        );

        if !email.attachments.is_empty() {
            let label = format!("{} attachment(s)", email.attachments.len());
            push_header(&mut lines, "Attachments", Some(&label), theme, width);
        }

        lines.push(String::new());

        match &email.body {
            Body::Html(htmlbody) => {
                lines.extend(html::render::render(
                    htmlbody.as_bytes(),
                    width,
                    style_mode,
                )?);
            }
            Body::Text(text) => {
                for raw in text.lines() {
                    let painted = theme.paint_value(raw);
                    if raw.is_empty() {
                        lines.push(String::new());
                    } else {
                        lines.extend(wrap_styled_words(&painted, width));
                    }
                }
            }
            Body::Empty => lines.push(theme.paint_muted("[no message body]")),
        }

        Ok(lines)
    }
}

/// Emit a `Label: value` header row, wrapping long values with a hanging
/// indent aligned under the value column. Absent headers emit nothing.
fn push_header(
    lines: &mut Vec<String>,
    name: &str,
    value: Option<&str>,
    theme: &PeekTheme,
    width: usize,
) {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return;
    };
    let prefix = format!("{name}: ");
    let indent = " ".repeat(display_width(&prefix));
    let budget = width.saturating_sub(display_width(&prefix)).max(1);
    let painted = theme.paint_value(value);
    let chunks = wrap_styled_words(&painted, budget);
    for (i, chunk) in chunks.into_iter().enumerate() {
        if i == 0 {
            lines.push(format!("{}{chunk}", theme.paint_label(&prefix)));
        } else {
            lines.push(format!("{indent}{chunk}"));
        }
    }
}
