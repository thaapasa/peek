//! Lazily-built structured pretty-print branch of a `ContentMode`.
//!
//! A structured file (JSON / YAML / TOML / XML, or SVG-as-XML) shows
//! two ways: the raw source, or a re-indented pretty form. The pretty
//! form needs the *whole* document — there is no streaming
//! pretty-printer — so it is parsed once, lazily, capped at
//! [`PRETTY_MAX_BYTES`], and its rendered lines are cached.
//!
//! [`PrettyView`] owns that branch: the one-shot parse, the size-cap /
//! parse-error fallback state, and the rendered-line cache (theme-keyed
//! when a syntax token highlights it, theme-independent otherwise).
//! `ContentMode` keeps the *view state* — whether the user is looking
//! at pretty vs raw right now — and does the windowing.

use std::rc::Rc;

use anyhow::Result;

use crate::input::InputSource;
use crate::theme::{PeekThemeName, StyleMode, ThemeManager};
use crate::viewer::highlight_lines;

/// Pretty-printing holds the whole document in memory — no streaming
/// pretty-printer exists. Above this size the branch refuses and the
/// raw streamed view takes over, so a multi-GB JSON-shaped log stays
/// openable.
pub(crate) const PRETTY_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Injected whole-document pretty-printer: raw text → re-indented text,
/// or a parse error whose `Display` becomes a warning. Boxed so the
/// shared mode carries no specific format / type-module dependency.
type PrettyPrinter = Box<dyn Fn(&str) -> Result<String>>;

/// Outcome of the one-shot parse.
enum Parsed {
    /// Pretty-printed text, ready to render.
    Text(String),
    /// Refused — the raw branch takes over. `cap_exceeded` true means
    /// the file was over [`PRETTY_MAX_BYTES`]; false is a read / parse
    /// error. The matching warning was already surfaced.
    Failed { cap_exceeded: bool },
}

/// Syntax-highlighting inputs for the rendered-line cache.
pub(crate) struct SyntaxRef<'a> {
    pub token: &'a str,
    pub theme_manager: &'a Rc<ThemeManager>,
}

/// The lazily-built pretty-print branch. See the module docs.
pub(crate) struct PrettyView {
    /// Whole-document pretty-printer, injected by the caller so this
    /// shared mode stays ignorant of any specific structured format or
    /// type module. Returns the re-indented text, or a parse error whose
    /// `Display` is surfaced as a warning.
    pretty_print: PrettyPrinter,
    /// Format display name for the parse-failure warning (e.g. "JSON").
    format_name: &'static str,
    /// Whether this branch should be the *default* view (pretty over raw)
    /// when the user hasn't forced `--raw`. False for lossy formats
    /// (JSONC / JSON5) that drop content on the pretty round-trip. Set by
    /// the caller that knows the format, so the foundation never reasons
    /// about which formats are lossy.
    starts_default: bool,
    /// `None` until the first parse attempt.
    parsed: Option<Parsed>,
    /// Rendered lines + the `(theme, colour)` they were produced for.
    /// Highlighted when a `SyntaxRef` is supplied; a plain split (which
    /// is theme-independent) otherwise — the key is then inert.
    rendered: Option<(PeekThemeName, StyleMode, Vec<String>)>,
}

impl PrettyView {
    pub(crate) fn new(
        pretty_print: impl Fn(&str) -> Result<String> + 'static,
        format_name: &'static str,
        starts_default: bool,
    ) -> Self {
        Self {
            pretty_print: Box::new(pretty_print),
            format_name,
            starts_default,
            parsed: None,
            rendered: None,
        }
    }

    /// Whether to open in pretty view by default (absent `--raw`).
    pub(crate) fn starts_default(&self) -> bool {
        self.starts_default
    }

    /// Parse the document on the first call; a no-op afterwards.
    /// `total_bytes` gates the size cap. Cap / read / parse warnings are
    /// appended to `warnings`.
    pub(crate) fn ensure_parsed(
        &mut self,
        source: &InputSource,
        total_bytes: u64,
        warnings: &mut Vec<String>,
    ) {
        if self.parsed.is_some() {
            return;
        }
        if total_bytes > PRETTY_MAX_BYTES {
            let mb = total_bytes / (1024 * 1024);
            warnings.push(format!(
                "file too large for pretty-print ({mb} MB > {} MB cap); showing raw source",
                PRETTY_MAX_BYTES / (1024 * 1024)
            ));
            self.parsed = Some(Parsed::Failed { cap_exceeded: true });
            return;
        }
        let raw = match source.read_text() {
            Ok(s) => s,
            Err(e) => {
                warnings.push(format!(
                    "couldn't load source for pretty-print ({e}); showing raw source"
                ));
                self.parsed = Some(Parsed::Failed {
                    cap_exceeded: false,
                });
                return;
            }
        };
        self.parsed = Some(match (self.pretty_print)(&raw) {
            Ok(text) => Parsed::Text(text),
            Err(e) => {
                warnings.push(format!(
                    "{} parse failed ({e}); showing raw source",
                    self.format_name
                ));
                Parsed::Failed {
                    cap_exceeded: false,
                }
            }
        });
    }

    /// Whether the parse succeeded — the pretty view is renderable.
    pub(crate) fn is_ready(&self) -> bool {
        matches!(self.parsed, Some(Parsed::Text(_)))
    }

    /// Whether the parse was attempted and refused — the user is locked
    /// to raw (`r` is inert, the status line shows "Raw (forced)").
    pub(crate) fn failed(&self) -> bool {
        matches!(self.parsed, Some(Parsed::Failed { .. }))
    }

    /// Whether the refusal was the size cap specifically.
    pub(crate) fn cap_exceeded(&self) -> bool {
        matches!(self.parsed, Some(Parsed::Failed { cap_exceeded: true }))
    }

    /// The pretty-printed text once parsed; `None` before parsing or
    /// after a failure. Used by the pipe path and the search scan.
    pub(crate) fn text(&self) -> Option<&str> {
        match &self.parsed {
            Some(Parsed::Text(t)) => Some(t),
            _ => None,
        }
    }

    /// Build the rendered-line cache for `(theme, style)` when it isn't
    /// current. Highlighted line-by-line when `syntax` is `Some`, a
    /// plain split otherwise. Caller must have confirmed [`is_ready`].
    pub(crate) fn ensure_rendered(
        &mut self,
        theme: PeekThemeName,
        style: StyleMode,
        syntax: Option<SyntaxRef<'_>>,
    ) -> Result<()> {
        // The plain split is theme-independent — only a highlighted
        // cache goes stale on a theme / colour change.
        let stale = match &self.rendered {
            None => true,
            Some((t, s, _)) => syntax.is_some() && (*t != theme || *s != style),
        };
        if !stale {
            return Ok(());
        }
        let text = self
            .text()
            .expect("ensure_rendered called on an unready PrettyView");
        let lines = match syntax {
            Some(s) => highlight_lines(text, s.token, s.theme_manager, theme, style)?,
            None => text.lines().map(String::from).collect(),
        };
        self.rendered = Some((theme, style, lines));
        Ok(())
    }

    /// The rendered lines, or `None` if [`ensure_rendered`](Self::ensure_rendered)
    /// hasn't run yet.
    pub(crate) fn rendered_lines(&self) -> Option<&[String]> {
        self.rendered.as_ref().map(|(_, _, lines)| lines.as_slice())
    }

    /// Drop the rendered-line cache (keeps the parse). Called on the
    /// raw/pretty toggle so a later re-entry rebuilds fresh.
    pub(crate) fn invalidate_render(&mut self) {
        self.rendered = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn source(text: &str) -> InputSource {
        InputSource::stdin(Bytes::copy_from_slice(text.as_bytes()))
    }

    /// Stand-in pretty-printer: splits on commas (so valid input spreads
    /// onto multiple lines) and fails on anything containing "not json".
    /// Exercises `PrettyView`'s state machine without a real parser.
    fn pv() -> PrettyView {
        PrettyView::new(
            |raw: &str| {
                if raw.contains("not json") {
                    anyhow::bail!("parse error");
                }
                Ok(raw.replace(',', ",\n"))
            },
            "JSON",
            true,
        )
    }

    #[test]
    fn parses_then_renders_plain_split() {
        let src = source(r#"{"b":2,"a":1}"#);
        let mut pv = pv();
        let mut warnings = Vec::new();
        pv.ensure_parsed(&src, src.read_bytes().unwrap().len() as u64, &mut warnings);

        assert!(pv.is_ready());
        assert!(warnings.is_empty());
        // Pretty-printed JSON spreads onto multiple lines.
        pv.ensure_rendered(PeekThemeName::IdeaDark, StyleMode::Plain, None)
            .unwrap();
        assert!(pv.rendered_lines().unwrap().len() > 1);
    }

    #[test]
    fn size_cap_refuses_and_warns() {
        let src = source("{}");
        let mut pv = pv();
        let mut warnings = Vec::new();
        // Lie about the size to trip the cap without a huge fixture.
        pv.ensure_parsed(&src, PRETTY_MAX_BYTES + 1, &mut warnings);

        assert!(!pv.is_ready());
        assert!(pv.failed());
        assert!(pv.cap_exceeded());
        assert!(warnings.iter().any(|w| w.contains("too large")));
    }

    #[test]
    fn parse_error_fails_without_cap_flag() {
        let src = source("not json at all");
        let mut pv = pv();
        let mut warnings = Vec::new();
        pv.ensure_parsed(&src, src.read_bytes().unwrap().len() as u64, &mut warnings);

        assert!(pv.failed());
        assert!(!pv.cap_exceeded());
        assert!(pv.text().is_none());
    }
}
