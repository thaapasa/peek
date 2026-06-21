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

use std::ops::Range;
use std::rc::Rc;

use anyhow::Result;

use crate::viewer::highlight_lines;
use crate::viewer::wrap_scroll::PrettyLines;
use peek_io::InputSource;
use peek_io::limits::Budget;
use peek_theme::{PeekThemeName, StyleMode, ThemeManager};

/// Pretty-printing holds the whole document in memory — no streaming
/// pretty-printer exists. Above this size the branch refuses and the
/// raw streamed view takes over, so a multi-GB JSON-shaped log stays
/// openable.
pub const PRETTY_MAX_BYTES: u64 = peek_io::limits::WHOLE_DOC_BYTES;

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
pub struct SyntaxRef<'a> {
    pub token: &'a str,
    pub theme_manager: &'a Rc<ThemeManager>,
}

/// The rendered-line cache. Highlighting produces ANSI-styled lines (a
/// second buffer, unavoidably distinct from the raw pretty text); the
/// un-highlighted view instead keeps byte spans into the single parsed
/// document, so it costs no second copy of the text.
enum Rendered {
    /// `(theme, style, lines)` — keyed so a theme / colour change
    /// rebuilds. Only the highlighted variant goes stale on theme.
    Highlighted(PeekThemeName, StyleMode, Vec<String>),
    /// Line spans into the parsed text (theme-independent).
    Plain(Vec<Range<usize>>),
}

/// Byte spans of each logical line in `text`, mirroring `str::lines`
/// (split on `\n`, drop a trailing `\r`). Spans index into `text`, so no
/// line content is copied.
fn line_spans(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            let end = if i > start && bytes[i - 1] == b'\r' {
                i - 1
            } else {
                i
            };
            spans.push(start..end);
            start = i + 1;
        }
    }
    if start < bytes.len() {
        let end = if bytes[bytes.len() - 1] == b'\r' && bytes.len() - 1 > start {
            bytes.len() - 1
        } else {
            bytes.len()
        };
        spans.push(start..end);
    }
    spans
}

/// The lazily-built pretty-print branch. See the module docs.
pub struct PrettyView {
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
    /// Rendered-line cache: highlighted lines (theme-keyed) when a
    /// `SyntaxRef` is supplied, borrowed plain spans otherwise.
    rendered: Option<Rendered>,
}

impl PrettyView {
    pub fn new(
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
    pub fn starts_default(&self) -> bool {
        self.starts_default
    }

    /// Parse the document on the first call; a no-op afterwards.
    /// `total_bytes` gates the size cap. Cap / read / parse warnings are
    /// appended to `warnings`.
    pub fn ensure_parsed(
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
        // Whole-doc transform; the size gate above already refused over cap.
        let raw = match source.read_text(Budget::WholeDoc("pretty-print source")) {
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
            // Pretty-printers can echo string values verbatim (e.g. a YAML
            // scalar holding an ESC byte), so neutralise terminal controls
            // before the text reaches the display / pipe path. Reuse the
            // owned string when already clean.
            Ok(text) => Parsed::Text(match peek_io::sanitize_terminal_controls(&text) {
                std::borrow::Cow::Borrowed(_) => text,
                std::borrow::Cow::Owned(clean) => clean,
            }),
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
    pub fn is_ready(&self) -> bool {
        matches!(self.parsed, Some(Parsed::Text(_)))
    }

    /// Whether the parse was attempted and refused — the user is locked
    /// to raw (`r` is inert, the status line shows "Raw (forced)").
    pub fn failed(&self) -> bool {
        matches!(self.parsed, Some(Parsed::Failed { .. }))
    }

    /// Whether the refusal was the size cap specifically.
    pub fn cap_exceeded(&self) -> bool {
        matches!(self.parsed, Some(Parsed::Failed { cap_exceeded: true }))
    }

    /// The pretty-printed text once parsed; `None` before parsing or
    /// after a failure. Used by the pipe path and the search scan.
    pub fn text(&self) -> Option<&str> {
        match &self.parsed {
            Some(Parsed::Text(t)) => Some(t),
            _ => None,
        }
    }

    /// Build the rendered-line cache for `(theme, style)` when it isn't
    /// current. Highlighted line-by-line when `syntax` is `Some`, a
    /// plain split otherwise. Caller must have confirmed [`is_ready`].
    pub fn ensure_rendered(
        &mut self,
        theme: PeekThemeName,
        style: StyleMode,
        syntax: Option<SyntaxRef<'_>>,
    ) -> Result<()> {
        // The plain split is theme-independent — only a highlighted
        // cache goes stale on a theme / colour change.
        let stale = match (&self.rendered, &syntax) {
            (Some(Rendered::Plain(_)), None) => false,
            (Some(Rendered::Highlighted(t, s, _)), Some(_)) => *t != theme || *s != style,
            _ => true,
        };
        if !stale {
            return Ok(());
        }
        let text = self
            .text()
            .expect("ensure_rendered called on an unready PrettyView");
        self.rendered = Some(match syntax {
            Some(s) => Rendered::Highlighted(
                theme,
                style,
                highlight_lines(text, s.token, s.theme_manager, theme, style)?,
            ),
            None => Rendered::Plain(line_spans(text)),
        });
        Ok(())
    }

    /// The rendered lines as a [`PrettyLines`] borrow, or `None` if
    /// [`ensure_rendered`](Self::ensure_rendered) hasn't run yet.
    pub fn rendered_lines(&self) -> Option<PrettyLines<'_>> {
        match &self.rendered {
            Some(Rendered::Highlighted(_, _, lines)) => Some(PrettyLines::Highlighted(lines)),
            Some(Rendered::Plain(spans)) => Some(PrettyLines::Plain {
                text: self.text()?,
                spans,
            }),
            None => None,
        }
    }

    /// Drop the rendered-line cache (keeps the parse). Called on the
    /// raw/pretty toggle so a later re-entry rebuilds fresh.
    pub fn invalidate_render(&mut self) {
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
    fn line_spans_match_str_lines() {
        // The plain view borrows these spans instead of copying lines, so
        // they must split identically to `str::lines` — including CRLF and
        // trailing-newline edges.
        for input in [
            "",
            "a",
            "a\n",
            "a\nb",
            "a\nb\n",
            "\n",
            "a\r\nb",
            "a\r\nb\r\n",
            "x\ry",
            "\r\n",
        ] {
            let got: Vec<&str> = line_spans(input)
                .iter()
                .map(|s| &input[s.clone()])
                .collect();
            let want: Vec<&str> = input.lines().collect();
            assert_eq!(got, want, "input {input:?}");
        }
    }

    #[test]
    fn parses_then_renders_plain_split() {
        let src = source(r#"{"b":2,"a":1}"#);
        let mut pv = pv();
        let mut warnings = Vec::new();
        pv.ensure_parsed(
            &src,
            src.read_bytes(Budget::Unbounded("test")).unwrap().len() as u64,
            &mut warnings,
        );

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
        pv.ensure_parsed(
            &src,
            src.read_bytes(Budget::Unbounded("test")).unwrap().len() as u64,
            &mut warnings,
        );

        assert!(pv.failed());
        assert!(!pv.cap_exceeded());
        assert!(pv.text().is_none());
    }
}
